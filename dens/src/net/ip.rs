pub mod nat;
pub use nat::{EasyNat, HardNat};
mod network;
pub use network::Ipv4Prefix;
pub use network::Network;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use bytes::{Buf, BufMut};
use quinn_udp::EcnCodepoint;

use super::checksum::{VALID_CHECKSUM, checksum_arr};

use super::error::ParseError;

#[derive(Clone)]
pub enum Header {
    V4 {
        src: Ipv4Addr,
        dst: Ipv4Addr,
        ecn: Option<EcnCodepoint>, // 2 bits
        payload_length: u16,
        protocol: u8,
    },
    V6 {
        src: Ipv6Addr,
        dst: Ipv6Addr,
        ecn: Option<EcnCodepoint>, // 2 bits
        payload_length: u16,
        protocol: u8,
    },
}
use Header::{V4, V6};
impl Header {
    #[must_use]
    pub fn is_v6(&self) -> bool {
        matches!(self, V6 { .. })
    }
    #[must_use]
    pub fn is_v4(&self) -> bool {
        matches!(self, V4 { .. })
    }

    /// # Panics
    ///
    /// Panics if the `src` and `dst` IP addresses have different versions (both
    /// must be IPv4 or both must be IPv6).
    #[must_use]
    pub fn new(
        src: IpAddr,
        dst: IpAddr,
        ecn: Option<EcnCodepoint>,
        payload_length: u16,
        protocol: u8,
    ) -> Self {
        match (src, dst) {
            (IpAddr::V4(src), IpAddr::V4(dst)) => V4 {
                src,
                dst,
                ecn,
                payload_length,
                protocol,
            },
            (IpAddr::V6(src), IpAddr::V6(dst)) => V6 {
                src,
                dst,
                ecn,
                payload_length,
                protocol,
            },
            (IpAddr::V4(_), IpAddr::V6(_)) | (IpAddr::V6(_), IpAddr::V4(_)) => {
                panic!("IP address versions do not match")
            }
        }
    }
    #[must_use]
    pub fn protocol(&self) -> u8 {
        match self {
            Header::V4 { protocol, .. } | Header::V6 { protocol, .. } => *protocol,
        }
    }
    #[must_use]
    pub fn payload_length(&self) -> u16 {
        match self {
            Header::V4 { payload_length, .. } | Header::V6 { payload_length, .. } => {
                *payload_length
            }
        }
    }
    #[must_use]
    pub fn ecn(&self) -> Option<EcnCodepoint> {
        match self {
            Header::V4 { ecn, .. } | Header::V6 { ecn, .. } => *ecn,
        }
    }

    /// (source, destination)
    #[must_use]
    pub fn get_ip_addrs(&self) -> (IpAddr, IpAddr) {
        match self {
            V4 { src, dst, .. } => (IpAddr::V4(*src), IpAddr::V4(*dst)),
            V6 { src, dst, .. } => (IpAddr::V6(*src), IpAddr::V6(*dst)),
        }
    }

    pub fn set_v4_address(&mut self, new_src: Ipv4Addr, new_dst: Ipv4Addr) {
        match self {
            V4 { src, dst, .. } => {
                *src = new_src;
                *dst = new_dst;
            }
            V6 {
                ecn,
                payload_length,
                protocol,
                ..
            } => {
                *self = V4 {
                    src: new_src,
                    dst: new_dst,
                    ecn: *ecn,
                    payload_length: *payload_length,
                    protocol: *protocol,
                };
            }
        }
    }

    pub fn set_v6_address(&mut self, new_src: Ipv6Addr, new_dst: Ipv6Addr) {
        match self {
            V6 { src, dst, .. } => {
                *src = new_src;
                *dst = new_dst;
            }
            V4 {
                ecn,
                payload_length,
                protocol,
                ..
            } => {
                *self = V6 {
                    src: new_src,
                    dst: new_dst,
                    ecn: *ecn,
                    payload_length: *payload_length,
                    protocol: *protocol,
                };
            }
        }
    }

    pub const IPV6_HEADER_LEN: usize = 10 * 4;
    pub const IPV6_HEADER_LEN_U16: u16 = const {
        assert!(Self::IPV6_HEADER_LEN <= u16::MAX as usize);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "we ensure it won't truncate with the above assertion"
        )]
        let casted = Self::IPV6_HEADER_LEN as u16;
        casted
    };

    pub const IPV4_HEADER_LEN: usize = 5 * 4;
    pub const IPV4_HEADER_LEN_U16: u16 = const {
        assert!(Self::IPV4_HEADER_LEN <= u16::MAX as usize);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "we ensure it won't truncate with the above assertion"
        )]
        let casted = Self::IPV4_HEADER_LEN as u16;
        casted
    };
    pub fn write_into_buf(&self, into: &mut impl BufMut) {
        match self {
            V4 {
                src,
                dst,
                ecn,
                payload_length,
                protocol,
            } => {
                Self::write_v4_into_buf(into, *src, *dst, ecn.as_ref(), *payload_length, *protocol);
            }
            Header::V6 {
                src,
                dst,
                ecn,
                payload_length,
                protocol,
            } => {
                Self::write_v6_into_buf(into, src, dst, ecn.as_ref(), *payload_length, *protocol);
            }
        }
    }
    fn write_v4_into_buf(
        into: &mut impl BufMut,
        src: Ipv4Addr,
        dst: Ipv4Addr,
        ecn: Option<&EcnCodepoint>,
        payload_length: u16,
        protocol: u8,
    ) {
        let mut header = [0u8; Self::IPV4_HEADER_LEN];

        let mut buf: &mut [u8] = &mut header;
        let version = 4;
        // 4 bits
        let internet_header_length = 5;
        // 4 bits
        let byte = (version << 4) | internet_header_length;

        buf.put_u8(byte);

        let differentiated_services_code_point = 0;
        // 6 bits
        let explicit_congestion_notification = ecn.map(|v| *v as u8).unwrap_or_default();
        // 2 bits
        let byte = (differentiated_services_code_point << 2) | explicit_congestion_notification;
        buf.put_u8(byte);
        buf.put_u16(payload_length + Self::IPV4_HEADER_LEN_U16);

        let identification = 0; // 16 bits
        buf.put_u16(identification);

        let flags = 0b010; // 3 bits: reserved, don't fragment, more fragments
        let fragment_offset = 0; // 13 bits
        let word: u16 = (flags << 13) | fragment_offset;
        buf.put_u16(word);
        let time_to_live = 64;
        // 8 bits
        buf.put_u8(time_to_live);

        buf.put_u8(protocol);

        let checksum = 0;
        // 16 bits; to be set later
        buf.put_u16(checksum);

        let source_address = src.octets();
        buf.put_slice(&source_address);

        let destination_address = dst.octets();
        buf.put_slice(&destination_address);

        // now that we have all of the bytes in bytes for the packet, we
        // calculate the IPv4 checksum
        let checksum = checksum_arr(&header);
        header[10..12].copy_from_slice(&(!checksum).to_be_bytes());
        into.put_slice(&header);
    }
    fn write_v6_into_buf(
        into: &mut impl BufMut,
        src: &Ipv6Addr,
        dst: &Ipv6Addr,
        ecn: Option<&EcnCodepoint>,
        payload_length: u16,
        protocol: u8,
    ) {
        let mut header = [0u8; Self::IPV6_HEADER_LEN];
        let mut buf: &mut [u8] = &mut header;
        let version: u32 = 6; // 4 bits
        let differentiated_services = 0; // 6 bits
        let explicit_congestion_notification = ecn.map(|v| *v as u8).unwrap_or_default(); // 2 bits
        let traffic_class = (differentiated_services << 6) | explicit_congestion_notification;
        let flow_label: u32 = 0; // 20 bits
        let row_one: u32 = (version << (8 + 20)) | ((u32::from(traffic_class)) << 20) | flow_label;
        buf.put_u32(row_one);

        buf.put_u16(payload_length);
        let next_header = protocol; // 8 bits
        buf.put_u8(next_header);

        let hop_limit = 64; // 8 bits
        buf.put_u8(hop_limit);

        let source_address = src.octets();
        buf.put_slice(&source_address);
        let destination_address = dst.octets();
        buf.put_slice(&destination_address);

        into.put_slice(&header);
    }
    /// # Errors
    ///
    /// * `ParseError::InvalidVersion(v)` - if the IP version is neither 4 nor 6.
    /// * `ParseError::NotEnoughForHeaders` - if the buffer contains insufficient data to parse the header.
    /// * `ParseError::InvalidIhl(ihl)` - if the IPv4 Internet Header Length is not 5.
    /// * `ParseError::InvalidChecksum(checksum)` - if the IPv4 header checksum is invalid.
    pub fn try_from_buf(from: &mut impl Buf) -> Result<Self, ParseError> {
        let first_byte = from.get_u8();
        let version = first_byte >> 4;
        match version {
            4 => Self::try_v4_from_buf(from, first_byte),
            6 => Self::try_v6_from_buf(from, first_byte),
            v => Err(ParseError::InvalidVersion(v))?,
        }
    }

    fn try_v4_from_buf(from: &mut impl Buf, first_byte: u8) -> Result<Self, ParseError> {
        let mut checksum_array = [0u8; Self::IPV4_HEADER_LEN];
        let mut checksum_buf: &mut [u8] = &mut checksum_array;
        checksum_buf.put_u8(first_byte);
        from.copy_to_slice(&mut checksum_array[1..]);
        let ipv4_checksum = checksum_arr(&checksum_array);
        if ipv4_checksum != VALID_CHECKSUM {
            Err(ParseError::InvalidChecksum(ipv4_checksum))?;
        }
        // already read first byte
        let mut from: &[u8] = &checksum_array[1..];

        let ihl = first_byte & 0b1111;
        if ihl != 5 {
            Err(ParseError::InvalidIhl(ihl))?;
        }
        if from.remaining() < Self::IPV4_HEADER_LEN - 1 {
            Err(ParseError::NotEnoughForHeaders {
                expected: Self::IPV4_HEADER_LEN_U16,
                had: from.remaining() + 1,
            })?;
        }
        let second_byte = from.get_u8();
        // can ignore dscp
        let ecn = EcnCodepoint::from_bits(second_byte);
        let total_length = from.get_u16();
        let _second_row = from.get_u32();
        let _ttl = from.get_u8();
        let protocol = from.get_u8();

        // can skip ipv4 checksum field, already verified it
        let _ipv4_checksum = from.get_u16();
        let src = Ipv4Addr::from_bits(from.get_u32());
        let dst = Ipv4Addr::from_bits(from.get_u32());
        Ok(V4 {
            src,
            dst,
            ecn,
            payload_length: total_length - Self::IPV4_HEADER_LEN_U16,
            protocol,
        })
    }

    fn try_v6_from_buf(from: &mut impl Buf, _first_byte: u8) -> Result<Header, ParseError> {
        // can ignore DS field
        // shift over 4 bits bc half of second byte is flow label
        if from.remaining() < Self::IPV6_HEADER_LEN - 1 {
            Err(ParseError::NotEnoughForHeaders {
                expected: Self::IPV6_HEADER_LEN_U16,
                had: from.remaining() + 1,
            })?;
        }
        let second_byte = from.get_u8() >> 4;
        let ecn = EcnCodepoint::from_bits(second_byte);
        let _rest_of_row = from.get_u16();
        let payload_length = from.get_u16();
        let next_header = from.get_u8();

        from.get_u8(); // skip hop limit
        let src = Ipv6Addr::from_bits(from.get_u128());
        let dst = Ipv6Addr::from_bits(from.get_u128());

        let out = V6 {
            src,
            dst,
            ecn,
            payload_length,
            protocol: next_header,
        };

        Ok(out)
    }
}

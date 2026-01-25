use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    num::{NonZero, NonZeroU16},
};

use quinn_udp::{EcnCodepoint, RecvMeta, Transmit};

use bytes::{Buf, BufMut, Bytes};

pub struct Packet {
    header: Header,
    data: Bytes,
}

impl From<Header> for RecvMeta {
    fn from(header: Header) -> Self {
        let (src, dst) = header.get_socket_addrs();
        RecvMeta {
            addr: src,
            len: header.payload_length as usize,
            stride: header.payload_length as usize,
            ecn: header.ecn,
            dst_ip: Some(dst.ip()),
        }
    }
}

impl Packet {
    #[inline]
    pub fn new(src: SocketAddr, dst: SocketAddr, data: &[u8], ecn: Option<EcnCodepoint>) -> Self {
        use IpAddrs::*;
        let (ip_addrs, (src_port, dst_port)) = match (src, dst) {
            (SocketAddr::V4(src), SocketAddr::V4(dst)) => (
                V4 {
                    src: *src.ip(),
                    dst: *dst.ip(),
                },
                (src.port(), dst.port()),
            ),
            (SocketAddr::V6(src), SocketAddr::V6(dst)) => (
                V6 {
                    src: *src.ip(),
                    dst: *dst.ip(),
                },
                (src.port(), dst.port()),
            ),
            (SocketAddr::V4(_), SocketAddr::V6(_)) => panic!("IP address versions do not match"),
            (SocketAddr::V6(_), SocketAddr::V4(_)) => panic!("IP address versions do not match"),
        };
        let mut header = Header {
            ecn,
            ip_addrs,
            src_port,
            dst_port,
            payload_length: data.len().try_into().unwrap(),
            checksum: None,
        };
        if header.ip_addrs.is_v6() {
            header.set_checksum(data);
        }
        let data = Bytes::copy_from_slice(data);

        Self { header, data }
    }
    /// will only set checksums if ipv6 packets.
    pub fn packets_from_transmit(transmit: Transmit<'_>, src_addr: SocketAddr) -> Vec<Self> {
        let Transmit {
            destination,
            ecn,
            contents,
            segment_size,
            src_ip,
        } = transmit;

        let segment_size = segment_size.unwrap_or(contents.len());
        let segment_range = 0..(contents.len() / segment_size);
        assert!(segment_size < u16::MAX.into());

        segment_range
            .map(|i| {
                Packet::new(
                    SocketAddr::new(src_ip.unwrap_or(src_addr.ip()), src_addr.port()),
                    destination,
                    &contents[(i * segment_size)..((i + 1) * segment_size)],
                    ecn,
                )
            })
            .collect()
    }

    pub fn set_checksum(&mut self) {
        self.header.set_checksum(&self.data);
    }
    pub fn check_checksum(&self) -> bool {
        self.header.check_checksum(&self.data)
    }

    pub fn into_bytes(mut self, into: &mut impl BufMut) {
        self.header.set_checksum(&self.data);
        self.header.to_bytes(into);
        into.put_slice(&self.data);
    }
    pub fn try_from_bytes(bytes: &mut Bytes) -> Result<Self, ParseError> {
        let header = Header::try_from_bytes(bytes)?;
        if header.payload_length as usize > bytes.len() {
            Err(ParseError::NotEnoughForData {
                expected: header.payload_length,
                had: bytes.len(),
            })?
        }
        let data = bytes.slice(0usize..header.payload_length as usize);
        Ok(Self { header, data })
    }
}
#[derive(Clone)]
pub(crate) struct Header {
    ip_addrs: IpAddrs,
    src_port: u16,
    dst_port: u16,
    payload_length: u16,
    checksum: Option<NonZeroU16>,
    ecn: Option<EcnCodepoint>,
}

#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    #[error("invalid IP version: {0}")]
    InvalidVersion(u8),
    #[error("invalid IHL: {0}. Might indicate presence of options field")]
    InvalidIhl(u8),
    #[error("invalid protocol number: {0}")]
    InvalidProtocolNumber(u8),
    #[error("invalid checksum: {0}")]
    InvalidChecksum(u16),
    #[error("unexpected next header: {0}. Might indicate IPV6 extension")]
    UnexpectedNextHeader(u8),
    #[error("not enough bytes for headers. Expected {expected} and found {had}")]
    NotEnoughForHeaders { expected: u16, had: usize },
    #[error("not enough bytes for body content. Expected {expected} and found {had}")]
    NotEnoughForData { expected: u16, had: usize },
}

#[derive(Clone, Copy)]
enum IpAddrs {
    V4 { src: Ipv4Addr, dst: Ipv4Addr },
    V6 { src: Ipv6Addr, dst: Ipv6Addr },
}

impl IpAddrs {
    fn is_v6(&self) -> bool {
        matches!(self, IpAddrs::V6 { .. })
    }
    #[allow(unused)]
    fn is_v4(&self) -> bool {
        matches!(self, IpAddrs::V4 { .. })
    }
    /// (source, destination)
    fn get_ip_addrs(&self) -> (IpAddr, IpAddr) {
        match self {
            IpAddrs::V4 { src, dst } => (IpAddr::V4(*src), IpAddr::V4(*dst)),
            IpAddrs::V6 { src, dst } => (IpAddr::V6(*src), IpAddr::V6(*dst)),
        }
    }
}

impl Header {
    pub const PROTOCOL: u8 = 0x11;
    pub const IPV6_HEADER_LEN: usize = 10 * 4;
    pub const IPV4_HEADER_LEN: usize = 5 * 4;
    pub const UDP_HEADER_LEN: usize = 2 * 4;

    pub const MAX_HEADER_LEN: usize = Self::IPV6_HEADER_LEN + Self::UDP_HEADER_LEN;

    /// Returns (source, destination) socket addresses.
    pub fn get_socket_addrs(&self) -> (SocketAddr, SocketAddr) {
        let (src, dst) = self.ip_addrs.get_ip_addrs();
        (
            SocketAddr::new(src, self.src_port),
            SocketAddr::new(dst, self.dst_port),
        )
    }
    #[inline]
    fn calculate_checksum(&self, data: &[u8]) -> u16 {
        let mut bytes = [0u8; Self::MAX_HEADER_LEN];
        let mut full_buf: &mut [u8] = &mut bytes;
        UdpPseudoHeader::new_from_header(self).write_into_buf(&mut full_buf);
        full_buf.put_u16(self.src_port);
        full_buf.put_u16(self.dst_port);
        full_buf.put_u16(self.udp_length());
        let (headers_chunks, remainder) = bytes.as_chunks::<2>();

        let headers_checksum = wrapping_sum(
            headers_chunks
                .iter()
                .map(|bytes: &[u8; 2]| u16::from_be_bytes(*bytes)),
        );
        assert_eq!(remainder.len(), 0); // we set bytes size to an even number
        let (data_chunks, remainder) = data.as_chunks::<2>();
        let data_checksum = wrapping_sum(
            data_chunks
                .iter()
                .map(|bytes: &[u8; 2]| u16::from_be_bytes(*bytes)),
        )
        .wrapping_add((*remainder.first().unwrap_or(&0) as u16) << 8);
        headers_checksum.wrapping_add(data_checksum)
    }

    pub fn set_checksum(&mut self, data: &[u8]) {
        if self.checksum.is_some() {
            return;
        }

        self.checksum = NonZero::new(!self.calculate_checksum(data));
    }

    fn udp_length(&self) -> u16 {
        self.payload_length + Self::UDP_HEADER_LEN as u16
    }
    #[inline]
    pub fn to_bytes(&self, into: &mut impl BufMut) {
        match self.ip_addrs {
            IpAddrs::V4 { src, dst } => {
                let mut header = [0u8; Self::IPV4_HEADER_LEN];

                let mut buf: &mut [u8] = &mut header;
                let version = 4; // 4 bits
                let internet_header_length = 5; // 4 bits
                let byte = (version << 4) | internet_header_length;

                buf.put_u8(byte);

                let differentiated_services_code_point = 0; // 6 bits
                let explicit_congestion_notification =
                    self.ecn.map(|v| v as u8).unwrap_or_default(); // 2 bits
                let byte =
                    (differentiated_services_code_point << 2) | explicit_congestion_notification;
                buf.put_u8(byte);

                buf.put_u16(self.udp_length() + Self::IPV4_HEADER_LEN as u16);

                let identification = 0; // 16 bits
                buf.put_u16(identification);

                let flags = 0b010; // 3 bits: reserved, don't fragment, more fragments
                let fragment_offset = 0; // 13 bits
                let word: u16 = (flags << 13) | fragment_offset;
                buf.put_u16(word);
                let time_to_live = 64; // 8 bits
                buf.put_u8(time_to_live);

                let protocol = Self::PROTOCOL;
                buf.put_u8(protocol);

                let checksum = 0; // 16 bits; to be set later
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
            IpAddrs::V6 { src, dst } => {
                let mut header = [0u8; Self::IPV6_HEADER_LEN];
                let mut buf: &mut [u8] = &mut header;
                let version = 4; // 4 bits
                let differentiated_services = 0; // 6 bits
                let explicit_congestion_notification =
                    self.ecn.map(|v| v as u8).unwrap_or_default(); // 2 bits
                let traffic_class =
                    (differentiated_services << 6) | explicit_congestion_notification;
                let flow_label = 0; // 20 bits
                let row_one: u32 = ((version as u32) << (8 + 20))
                    | ((traffic_class as u32) << 20)
                    | flow_label as u32;
                buf.put_u32(row_one);

                let payload_length = self.payload_length; // 16 bits
                buf.put_u16(payload_length);
                let next_header = Self::PROTOCOL; // 8 bits
                buf.put_u8(next_header);

                let hop_limit = 64; // 8 bits
                buf.put_u8(hop_limit);

                let source_address = src.octets();
                buf.put_slice(&source_address);
                let destination_address = dst.octets();
                buf.put_slice(&destination_address);

                into.put_slice(&header);
            }
        };
        let mut udp_header = [0u8; Self::UDP_HEADER_LEN];
        let mut buf: &mut [u8] = &mut udp_header;
        buf.put_u16(self.src_port);
        buf.put_u16(self.dst_port);
        buf.put_u16(self.udp_length());
        buf.put_u16(self.checksum.map(Into::into).unwrap_or_default());
        into.put_slice(&udp_header);
    }

    pub fn try_from_bytes(from: &mut Bytes) -> Result<Self, ParseError> {
        let ipv4_checksum = checksum_arr(from.slice(0..20).as_array::<20>().unwrap());
        let first_byte = from.get_u8();
        let version = first_byte >> 4;
        let (ecn, ip_addrs, payload_length) = match version {
            4 => {
                if ipv4_checksum != 0xffff {
                    Err(ParseError::InvalidChecksum(ipv4_checksum))?;
                }
                let ihl = first_byte & 0b1111;
                if ihl != 5 {
                    Err(ParseError::InvalidIhl(ihl))?;
                }
                if from.len() < Self::IPV4_HEADER_LEN - 1 {
                    Err(ParseError::NotEnoughForHeaders {
                        expected: Self::IPV4_HEADER_LEN as u16,
                        had: from.len() + 1,
                    })?
                }
                let second_byte = from.get_u8();
                // can ignore dscp
                let ecn = EcnCodepoint::from_bits(second_byte);
                let total_length = from.get_u16();
                let _second_row = from.get_u32();
                let _ttl = from.get_u8();
                let protocol = from.get_u8();
                if protocol != Self::PROTOCOL {
                    Err(ParseError::InvalidProtocolNumber(protocol))?
                }
                // can skip ipv4 checksum field, already verified it
                let _ipv4_checksum = from.get_u16();
                let src = Ipv4Addr::from_bits(from.get_u32());
                let dst = Ipv4Addr::from_bits(from.get_u32());
                (
                    ecn,
                    IpAddrs::V4 { src, dst },
                    total_length - (Self::IPV4_HEADER_LEN + Self::UDP_HEADER_LEN) as u16,
                )
            }
            6 => {
                // can ignore DS field
                // shift over 4 bits bc half of second byte is flow label
                if from.len() < Self::IPV6_HEADER_LEN - 1 {
                    Err(ParseError::NotEnoughForHeaders {
                        expected: Self::IPV6_HEADER_LEN as u16,
                        had: from.len() + 1,
                    })?
                }
                let second_byte = from.get_u8() >> 4;
                let ecn = EcnCodepoint::from_bits(second_byte);
                let _rest_of_row = from.get_u16();
                let payload_length = from.get_u16();
                let next_header = from.get_u8();
                if next_header != Self::PROTOCOL {
                    Err(ParseError::UnexpectedNextHeader(next_header))?
                }
                from.get_u8(); // skip hop limit
                let src_addr = Ipv6Addr::from_bits(from.get_u128());
                let dst_addr = Ipv6Addr::from_bits(from.get_u128());

                (
                    ecn,
                    IpAddrs::V6 {
                        src: src_addr,
                        dst: dst_addr,
                    },
                    payload_length,
                )
            }
            v => Err(ParseError::InvalidVersion(v))?,
        };
        let src_port = from.get_u16();
        let dst_port = from.get_u16();
        let udp_length = from.get_u16();
        assert_eq!(udp_length - Self::UDP_HEADER_LEN as u16, payload_length);
        let checksum = NonZero::new(from.get_u16());

        Ok(Self {
            ip_addrs,
            src_port,
            dst_port,
            payload_length,
            checksum,
            ecn,
        })
    }
    pub fn check_checksum(&self, data: &[u8]) -> bool {
        let Some(checksum) = self.checksum else {
            return true; // we skip the check
        };
        self.calculate_checksum(data) + u16::from(checksum) == 0xffff
    }
}

pub struct Udpv6PseudoHeader {
    src_addr: [u8; 16],
    dst_addr: [u8; 16],
    udp_length: u16,
}

pub struct Udpv4PseudoHeader {
    src_addr: [u8; 4],
    dst_addr: [u8; 4],
    udp_length: u16,
}

pub enum UdpPseudoHeader {
    V4(Udpv4PseudoHeader),
    V6(Udpv6PseudoHeader),
}

#[inline]
fn wrapping_sum(nums: impl IntoIterator<Item = u16>) -> u16 {
    let mut out: u16 = 0;
    for num in nums {
        out = out.wrapping_add(num);
    }
    out
}

fn checksum_arr<const LEN: usize>(arr: &[u8; LEN]) -> u16 {
    assert!(LEN.is_multiple_of(2));
    wrapping_sum(
        arr.chunks_exact(2)
            .map(|arr| u16::from_be_bytes([arr[0], arr[1]])),
    )
}

impl UdpPseudoHeader {
    pub fn new_from_header(udp_header: &Header) -> Self {
        match &udp_header.ip_addrs {
            IpAddrs::V4 {
                src: src_addr,
                dst: dst_addr,
            } => Self::V4(Udpv4PseudoHeader {
                src_addr: src_addr.octets(),
                dst_addr: dst_addr.octets(),
                udp_length: udp_header.udp_length(),
            }),
            IpAddrs::V6 {
                src: src_addr,
                dst: dst_addr,
            } => Self::V6(Udpv6PseudoHeader {
                src_addr: src_addr.octets(),
                dst_addr: dst_addr.octets(),
                udp_length: udp_header.udp_length(),
            }),
        }
    }

    pub fn write_into_buf(&self, buf: &mut impl BufMut) {
        match self {
            UdpPseudoHeader::V4(header) => buf.put_slice(&header.to_bytes()),
            UdpPseudoHeader::V6(header) => buf.put_slice(&header.to_bytes()),
        }
    }
}

impl Udpv4PseudoHeader {
    #[inline]
    pub fn to_bytes(&self) -> [u8; 11] {
        let mut arr = [0u8; 11];
        let mut buf: &mut [u8] = &mut arr;
        buf.put_slice(&self.src_addr);
        buf.put_slice(&self.dst_addr);
        buf.put_u8(Header::PROTOCOL);
        buf.put_u16(self.udp_length);
        arr
    }
}

impl Udpv6PseudoHeader {
    #[inline]
    pub fn to_bytes(&self) -> [u8; 35] {
        let mut arr = [0u8; 35];
        let mut buf: &mut [u8] = &mut arr;
        buf.put_slice(&self.src_addr);
        buf.put_slice(&self.dst_addr);
        buf.put_u8(Header::PROTOCOL);
        buf.put_u16(self.udp_length);
        arr
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;

    use crate::Packet;

    #[test]
    fn checksum() {
        let mut val = Packet::new(
            "127.0.0.1:8080".parse().unwrap(),
            "127.0.0.1:3000".parse().unwrap(),
            b"hello world!",
            None,
        );
        val.set_checksum();
        assert!(val.check_checksum());
        let mut bytes = BytesMut::new();
        val.into_bytes(&mut bytes);
        let val2 = Packet::try_from_bytes(&mut bytes.freeze()).unwrap();
        assert!(val2.check_checksum());
    }
}

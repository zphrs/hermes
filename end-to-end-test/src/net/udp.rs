use std::{
    hint::unreachable_unchecked,
    net::{IpAddr, SocketAddr},
    num::{NonZero, NonZeroU16},
};

use quinn_udp::{EcnCodepoint, RecvMeta, Transmit};

use bytes::{Buf, BufMut, Bytes};

use super::{
    checksum::{VALID_CHECKSUM, wrapping_sum},
    error::ParseError,
};

use super::ip;

pub struct Packet {
    header: Header,
    data: Bytes,
}

impl From<Header> for RecvMeta {
    fn from(header: Header) -> Self {
        let (src, dst) = header.get_socket_addrs();
        RecvMeta {
            addr: src,
            len: header.payload_length() as usize,
            stride: header.payload_length() as usize,
            ecn: header.ip_header.ecn(),
            dst_ip: Some(dst.ip()),
        }
    }
}

impl Packet {
    #[inline]
    pub fn new(src: SocketAddr, dst: SocketAddr, data: &[u8], ecn: Option<EcnCodepoint>) -> Self {
        let ip_header = ip::Header::new(
            src.ip(),
            dst.ip(),
            ecn,
            u16::try_from(data.len()).expect("data should be less than 65536 bytes")
                + Header::UDP_HEADER_LEN as u16,
            Header::PROTOCOL,
        );
        let mut header = Header {
            ip_header,
            src_port: src.port(),
            dst_port: dst.port(),
            checksum: None,
        };
        if header.ip_header.is_v6() {
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

    pub fn write_into_buf(mut self, into: &mut impl BufMut) {
        self.header.set_checksum(&self.data);
        self.header.write_into_buf(into);
        into.put_slice(&self.data);
    }
    pub fn try_from_bytes(bytes: &mut Bytes) -> Result<Self, ParseError> {
        let header = Header::try_from_buf(bytes)?;
        if header.payload_length() as usize > bytes.len() {
            Err(ParseError::NotEnoughForData {
                expected: header.payload_length(),
                had: bytes.len(),
            })?
        }
        let data = bytes.slice(0usize..header.payload_length() as usize);
        Ok(Self { header, data })
    }

    pub fn body(&self) -> Bytes {
        self.data.clone()
    }
}
#[derive(Clone)]
pub(crate) struct Header {
    ip_header: ip::Header,
    src_port: u16,
    dst_port: u16,
    checksum: Option<NonZeroU16>,
}

impl Header {
    pub const PROTOCOL: u8 = 0x11;
    pub const UDP_HEADER_LEN: usize = 2 * 4;

    pub const MAX_HEADER_LEN: usize = ip::Header::IPV6_HEADER_LEN + Self::UDP_HEADER_LEN;

    pub fn payload_length(&self) -> u16 {
        self.ip_header.payload_length() - Self::UDP_HEADER_LEN as u16
    }

    /// Returns (source, destination) socket addresses.
    pub fn get_socket_addrs(&self) -> (SocketAddr, SocketAddr) {
        let (src, dst) = self.ip_header.get_ip_addrs();
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
        let mapped_chunks = headers_chunks
            .iter()
            .map(|bytes: &[u8; 2]| u16::from_be_bytes(*bytes));
        let headers_checksum = wrapping_sum(mapped_chunks);
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
        self.ip_header.payload_length()
    }
    #[inline]
    pub fn write_into_buf(&self, into: &mut impl BufMut) {
        self.ip_header.write_into_buf(into);
        let mut udp_header = [0u8; Self::UDP_HEADER_LEN];
        let mut buf: &mut [u8] = &mut udp_header;
        buf.put_u16(self.src_port);
        buf.put_u16(self.dst_port);
        buf.put_u16(self.udp_length());
        buf.put_u16(self.checksum.map(Into::into).unwrap_or_default());
        into.put_slice(&udp_header);
    }

    pub fn try_from_buf(from: &mut impl Buf) -> Result<Self, ParseError> {
        let ip_header = ip::Header::try_from_buf(from)?;
        let src_port = from.get_u16();
        let dst_port = from.get_u16();
        let udp_length = from.get_u16();
        assert_eq!(udp_length, ip_header.payload_length());
        let checksum = NonZero::new(from.get_u16());

        Ok(Self {
            ip_header,
            src_port,
            dst_port,
            checksum,
        })
    }
    pub fn check_checksum(&self, data: &[u8]) -> bool {
        let Some(checksum) = self.checksum else {
            return true; // we skip the check
        };
        self.calculate_checksum(data) + u16::from(checksum) == VALID_CHECKSUM
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

pub(super) enum UdpPseudoHeader {
    V4(Udpv4PseudoHeader),
    V6(Udpv6PseudoHeader),
}

impl UdpPseudoHeader {
    pub fn new_from_header(udp_header: &Header) -> Self {
        match &udp_header.ip_header.get_ip_addrs() {
            (IpAddr::V4(src), IpAddr::V4(dst)) => Self::V4(Udpv4PseudoHeader {
                src_addr: src.octets(),
                dst_addr: dst.octets(),
                udp_length: udp_header.udp_length(),
            }),
            (IpAddr::V6(src_addr), IpAddr::V6(dst_addr)) => Self::V6(Udpv6PseudoHeader {
                src_addr: src_addr.octets(),
                dst_addr: dst_addr.octets(),
                udp_length: udp_header.udp_length(),
            }),

            (IpAddr::V4(_), IpAddr::V6(_)) =>
            // SAFETY: header is either a v4 header or a v6 header.
            // A hybrid v4/v6 header does not exist
            unsafe { unreachable_unchecked() },
            (IpAddr::V6(_), IpAddr::V4(_)) =>
            // SAFETY: header is either a v4 header or a v6 header.
            // A hybrid v4/v6 header does not exist
            unsafe { unreachable_unchecked() },
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
    pub fn to_bytes(&self) -> [u8; 12] {
        let mut arr = [0u8; 12];
        let mut buf: &mut [u8] = &mut arr;
        buf.put_slice(&self.src_addr);
        buf.put_slice(&self.dst_addr);
        buf.put_u8(0);
        buf.put_u8(Header::PROTOCOL);
        buf.put_u16(self.udp_length);
        arr
    }
}

impl Udpv6PseudoHeader {
    #[inline]
    pub fn to_bytes(&self) -> [u8; 36] {
        let mut arr = [0u8; 36];
        let mut buf: &mut [u8] = &mut arr;
        buf.put_slice(&self.src_addr);
        buf.put_slice(&self.dst_addr);
        buf.put_u8(0);
        buf.put_u8(Header::PROTOCOL);
        buf.put_u16(self.udp_length);
        arr
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;

    use super::Packet;

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
        val.write_into_buf(&mut bytes);
        let val2 = Packet::try_from_bytes(&mut bytes.freeze()).unwrap();
        assert!(val2.check_checksum());

        let val_ipv6 = Packet::new(
            "[2001:db8:85a3::8a2e:370:7334]:8080".parse().unwrap(),
            "[2001:db8:85a3::8a2e:370:7334]:3000".parse().unwrap(),
            b"hello world!",
            None,
        ); // ipv6 makes udp checksum mandatory
        assert!(val_ipv6.check_checksum());

        let mut bytes = BytesMut::new();
        val_ipv6.write_into_buf(&mut bytes);
        let from_bytes = Packet::try_from_bytes(&mut bytes.freeze()).unwrap();
        assert!(from_bytes.check_checksum());
    }
}

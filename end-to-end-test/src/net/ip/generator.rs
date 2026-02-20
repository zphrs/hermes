use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug)]
pub(crate) enum Generator {
    /// the next ip addr without the network prefix, as u32
    V4(u32),
    /// the next ip addr without the network prefix, as u128
    V6(u128),
}

impl Default for Generator {
    fn default() -> Self {
        Self::V4(1)
    }
}

impl Generator {
    pub(crate) fn next(&mut self) -> IpAddr {
        match self {
            Self::V4(next) => {
                let host = *next;
                *next = next.wrapping_add(1);

                let a = (host >> 8) as u8;
                let b = (host & 0xFF) as u8;

                IpAddr::V4(Ipv4Addr::new(192, 168, a, b))
            }
            Self::V6(next) => {
                let host = *next;
                *next = next.wrapping_add(1);

                let a = ((host >> 48) & 0xffff) as u16;
                let b = ((host >> 32) & 0xffff) as u16;
                let c = ((host >> 16) & 0xffff) as u16;
                let d = (host & 0xffff) as u16;

                IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, a, b, c, d))
            }
        }
    }
}

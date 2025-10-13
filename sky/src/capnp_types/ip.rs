use std::net::{IpAddr, Ipv6Addr};

use super::InitWith;
use crate::sky_capnp::{ip, ipv6};

impl From<ipv6::Reader<'_>> for Ipv6Addr {
    fn from(value: ipv6::Reader<'_>) -> Self {
        let left = value.get_left() as u128;
        let right = value.get_right() as u128;
        let total = right + (left << 64);

        Ipv6Addr::from_bits(total)
    }
}

impl TryFrom<ip::Reader<'_>> for IpAddr {
    type Error = capnp::Error;

    fn try_from(value: ip::Reader<'_>) -> Result<Self, Self::Error> {
        let ip = match value.which()? {
            ip::Which::V4(num) => IpAddr::V4(num.into()),
            ip::Which::V6(v6) => IpAddr::V6(v6?.into()),
        };
        Ok(ip)
    }
}

impl<'a> InitWith<'a, Ipv6Addr> for ipv6::Builder<'a> {
    fn with(mut self, other: &Ipv6Addr) -> Result<(), capnp::Error> {
        let bits = other.to_bits();
        let right = (bits >> 64) as u64;
        let left = bits as u64;
        self.set_left(left);
        self.set_right(right);
        Ok(())
    }
}

impl<'a> InitWith<'a, IpAddr> for ip::Builder<'a> {
    fn with(mut self, other: &IpAddr) -> Result<(), capnp::Error> {
        match other {
            IpAddr::V4(ipv4_addr) => Ok(self.set_v4(ipv4_addr.to_bits())),
            IpAddr::V6(ipv6_addr) => self.init_v6().with(ipv6_addr),
        }
    }
}

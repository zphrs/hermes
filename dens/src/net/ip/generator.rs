use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub(crate) type Ipv4Generator = crate::net::ip::Generator<super::generator::Ipv4>;

pub(crate) type Ipv6Generator = crate::net::ip::Generator<super::generator::Ipv6>;

#[derive(Debug, Default)]
pub(crate) struct Generator<Inner: IpGenerator + Default>(Inner);

trait IpGenerator {
    fn next(&mut self) -> impl Into<IpAddr>;
}

pub(crate) struct Ipv4(u32);

impl Default for Ipv4 {
    fn default() -> Self {
        Self(1)
    }
}

pub(crate) struct Ipv6(u128);

impl From<Ipv6> for Generator<Ipv6> {
    fn from(value: Ipv6) -> Self {
        Self(value)
    }
}

impl From<Ipv4> for Generator<Ipv4> {
    fn from(value: Ipv4) -> Self {
        Self(value)
    }
}

impl Default for Ipv6 {
    fn default() -> Self {
        Self(1)
    }
}

impl Ipv6 {
    pub fn next(&mut self) -> Ipv6Addr {
        let host = self.0;
        self.0 = self.0.wrapping_add(1);

        let a = ((host >> 48) & 0xffff) as u16;
        let b = ((host >> 32) & 0xffff) as u16;
        let c = ((host >> 16) & 0xffff) as u16;
        let d = (host & 0xffff) as u16;

        Ipv6Addr::new(0xfe80, 0, 0, 0, a, b, c, d)
    }
}

impl IpGenerator for Ipv6 {
    fn next(&mut self) -> impl Into<IpAddr> {
        self.next()
    }
}

impl Ipv4 {
    pub fn next(&mut self) -> Ipv4Addr {
        let host = self.0;
        self.0 = self.0.wrapping_add(1);

        let a = (host >> 8) as u8;
        let b = (host & 0xFF) as u8;

        Ipv4Addr::new(192, 168, a, b)
    }
}

impl IpGenerator for Ipv4 {
    fn next(&mut self) -> impl Into<IpAddr> {
        self.next()
    }
}

impl<Inner: IpGenerator + Default> Generator<Inner> {
    pub(crate) fn next(&mut self) -> IpAddr {
        self.0.next().into()
    }
}

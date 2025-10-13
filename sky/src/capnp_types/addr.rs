use std::net::{IpAddr, SocketAddr};

use crate::{capnp_types::InitWith, sky_capnp};

impl<'a> InitWith<'a, SocketAddr> for sky_capnp::socket_addr::Builder<'a> {
    fn with(mut self, other: &SocketAddr) -> Result<(), capnp::Error> {
        self.reborrow().init_ip().with(&other.ip())?;
        self.set_port(other.port());
        Ok(())
    }
}

impl TryFrom<sky_capnp::socket_addr::Reader<'_>> for SocketAddr {
    type Error = capnp::Error;

    fn try_from(value: sky_capnp::socket_addr::Reader<'_>) -> Result<Self, Self::Error> {
        let ip: IpAddr = value.get_ip()?.try_into()?;
        let port: u16 = value.get_port();
        Ok(Self::from((ip, port)))
    }
}

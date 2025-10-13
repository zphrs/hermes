use std::net::SocketAddr;

use crate::{capnp_types::InitWith, node::Node, sky_capnp};

impl<'a> InitWith<'a, Node> for sky_capnp::node::Builder<'a> {
    fn with(self, other: &Node) -> Result<(), capnp::Error> {
        self.init_addr().with(&other.addr())?;
        Ok(())
    }
}

impl TryFrom<sky_capnp::node::Reader<'_>> for Node {
    type Error = capnp::Error;

    fn try_from(value: sky_capnp::node::Reader<'_>) -> Result<Self, Self::Error> {
        let addr = SocketAddr::try_from(value.get_addr()?)?;
        Ok(Self::new(addr))
    }
}

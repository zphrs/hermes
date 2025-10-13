use std::net::{IpAddr, SocketAddr};

use kademlia::{HasId, id::Id};
use sha2::{Digest, Sha256};
use tokio::net::ToSocketAddrs;

use crate::sky_capnp::node;

#[derive(Clone, Debug)]
pub struct Node {
    addr: SocketAddr,
    id: Id<32>,
}

impl Node {
    pub fn new(socket_addr: SocketAddr) -> Self {
        let id: [u8; 32] = Sha256::digest(socket_addr.ip().to_string().as_bytes()).into();
        Self {
            addr: socket_addr,
            id: id.into(),
        }
    }

    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }
}

impl From<Node> for SocketAddr {
    fn from(value: Node) -> Self {
        value.addr
    }
}
impl HasId<32> for Node {
    fn id(&self) -> &Id<32> {
        &self.id
    }
}

use super::super::InitWith;
use crate::{
    capnp_types::Id,
    sky_capnp::{find_node_request, peer_request},
};

impl<'a> InitWith<'a, PeerRequest> for peer_request::Builder<'a> {
    fn with(mut self, other: &PeerRequest) -> Result<(), capnp::Error> {
        match other {
            PeerRequest::Ping => Ok(self.set_ping(())),
            PeerRequest::FindNode(find_node_request) => {
                self.init_find_node().with(find_node_request)
            }
        }
    }
}

pub enum PeerRequest {
    Ping,
    FindNode(FindNodeRequest),
}

pub struct FindNodeRequest {
    target_addr: Id,
}

impl FindNodeRequest {
    pub fn new(target_addr: Id) -> Self {
        Self { target_addr }
    }
}

impl<'a> InitWith<'a, FindNodeRequest> for find_node_request::Builder<'a> {
    fn with(self, other: &FindNodeRequest) -> Result<(), capnp::Error> {
        self.init_target_addr().with(&other.target_addr)?;
        Ok(())
    }
}

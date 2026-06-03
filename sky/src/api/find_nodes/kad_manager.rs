use crate::quinn_transport;

use super::KadHandler;
use shared_schema::SkyNode;

#[derive(Clone)]
pub struct KadRpcManager(kademlia::RpcManager<SkyNode, KadHandler, 32, 20>);

impl KadRpcManager {
    pub fn new(transport: quinn_transport::Transport, local_node: SkyNode) -> Self {
        Self(kademlia::RpcManager::new(
            KadHandler::new(transport.clone()),
            local_node.clone(),
        ))
    }
    pub fn into_inner(self) -> kademlia::RpcManager<SkyNode, KadHandler, 32, 20> {
        self.0
    }

    pub(crate) fn inner(&self) -> &kademlia::RpcManager<SkyNode, KadHandler, 32, 20> {
        &self.0
    }
}

use std::{borrow::Cow, convert::Infallible, time::Duration};

use shared_schema::SkyNode;
use tracing::trace;

use crate::{
    quinn_transport,
    request_handler::{FindNodesRequest, FindNodesResponse},
};

use super::KadHandler;

#[derive(Clone)]
pub struct FindNodesMethod<'a> {
    rpc_manager: kademlia::RpcManager<SkyNode, KadHandler, 32, 20>,
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> FindNodesMethod<'a> {
    pub fn new(transport: &quinn_transport::Transport, me: SkyNode) -> Self {
        let rpc_manager = kademlia::RpcManager::new(
            KadHandler {
                transport: transport.clone(),
            },
            me.clone(),
        );
        Self {
            rpc_manager,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn local_node(&self) -> &SkyNode {
        self.rpc_manager.local_node()
    }

    pub async fn refresh_stale_buckets(&self, duration: &Duration) {
        self.rpc_manager.refresh_stale_buckets(duration).await
    }

    pub async fn add_nodes(&self, nodes: impl IntoIterator<Item = SkyNode>) {
        self.rpc_manager.add_nodes(nodes).await
    }

    pub async fn join_network(&self) {
        self.rpc_manager.join_network().await
    }
}

impl<'a> rpc::Method for FindNodesMethod<'a> {
    type Req = FindNodesRequest<'a>;

    type Res = FindNodesResponse;

    type Error = Infallible;
}

impl<'a> rpc::Call for FindNodesMethod<'a> {
    #[tracing::instrument(skip(self))]
    async fn call(&mut self, value: Self::Req) -> Result<FindNodesResponse, Infallible> {
        let sky_id: kademlia::Id<32> = value.sky_id.into();
        let owned = value.from.map(Cow::into_owned);
        let out = self.rpc_manager.find_node(owned, &sky_id).await;

        trace!(?out);
        Ok(out.into())
    }
}

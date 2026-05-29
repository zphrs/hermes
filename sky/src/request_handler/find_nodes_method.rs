mod kad_handler;
mod kad_manager;
use kad_handler::KadHandler;
pub use kad_manager::KadRpcManager;

use std::{convert::Infallible, time::Duration};

use max_sized_vec::MaxSizedVec;
use maxlen::MaxLen;
use shared_schema::{SkyNode, sky_node::SkyId};
use tracing::{trace, warn};

use crate::client::SkyOrEarth;

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
pub struct FindNodesRequest {
    #[n(0)]
    pub sky_id: SkyId,
}

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
pub struct FindNodesResponse {
    #[n(0)]
    pub sky_nodes: MaxSizedVec<SkyNode, 20>,
}

impl FindNodesResponse {
    pub fn inner(&self) -> &arrayvec::ArrayVec<SkyNode, 20> {
        self.sky_nodes.inner()
    }

    pub fn into_inner(self) -> arrayvec::ArrayVec<SkyNode, 20> {
        self.sky_nodes.into_inner()
    }
}

impl From<Vec<SkyNode>> for FindNodesResponse {
    fn from(sky_nodes: Vec<SkyNode>) -> Self {
        Self {
            sky_nodes: sky_nodes.into_iter().collect(),
        }
    }
}

impl From<FindNodesResponse> for Vec<SkyNode> {
    fn from(res: FindNodesResponse) -> Self {
        res.sky_nodes.into_inner().into_iter().collect()
    }
}

#[derive(Clone)]
pub struct FindNodesMethod {
    rpc_manager: kademlia::RpcManager<SkyNode, KadHandler, 32, 20>,
    remote: Option<SkyNode>,
}

impl<'a> FindNodesMethod {
    pub fn from_manager(rpc_manager: &KadRpcManager, from: Option<SkyNode>) -> Self {
        Self {
            rpc_manager: rpc_manager.clone().into_inner(),
            remote: from,
        }
    }

    pub async fn on_ping(&self, from: SkyOrEarth) {
        let Some(sky_node) = from.as_sky() else {
            return;
        };
        warn!("should make sure that pinged nodes have their last_reached timer reset");
        self.rpc_manager
            .add_nodes_without_removing([sky_node.clone()].into_iter())
            .await;
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

impl rpc::Method for FindNodesMethod {
    type Req = FindNodesRequest;

    type Res = FindNodesResponse;

    type Error = Infallible;
}

impl rpc::Call for FindNodesMethod {
    #[tracing::instrument(skip(self, replier))]
    async fn call<T: futures_io::AsyncWrite + Unpin + Send + Sync, TransportError>(
        &mut self,
        replier: rpc::Replier<'_, T, Self>,
        value: Self::Req,
    ) -> Result<rpc::ReplyReceipt<Self::Res>, rpc::ClientError<TransportError, Self::Error>> {
        let sky_id: kademlia::Id<32> = value.sky_id.into();
        let out = self
            .rpc_manager
            .find_node(self.remote.clone(), &sky_id)
            .await;

        trace!(?out);
        replier.reply(out.into()).await
    }
}

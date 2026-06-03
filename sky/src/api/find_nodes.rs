mod kad_handler;
mod kad_manager;
use kad_handler::KadHandler;
pub use kad_manager::KadRpcManager;

use std::convert::Infallible;

use max_sized_vec::MaxSizedVec;
use maxlen::MaxLen;
use shared_schema::{SkyNode, sky_node::SkyId};
use tracing::trace;

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
pub struct Request {
    #[n(0)]
    pub sky_id: SkyId,
}

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
pub struct Response {
    #[n(0)]
    pub sky_nodes: MaxSizedVec<SkyNode, 20>,
}

impl Response {
    pub fn inner(&self) -> &arrayvec::ArrayVec<SkyNode, 20> {
        self.sky_nodes.inner()
    }

    pub fn into_inner(self) -> arrayvec::ArrayVec<SkyNode, 20> {
        self.sky_nodes.into_inner()
    }
}

impl From<Vec<SkyNode>> for Response {
    fn from(sky_nodes: Vec<SkyNode>) -> Self {
        Self {
            sky_nodes: sky_nodes.into_iter().collect(),
        }
    }
}

impl From<Response> for Vec<SkyNode> {
    fn from(res: Response) -> Self {
        res.sky_nodes.into_inner().into_iter().collect()
    }
}

#[derive(Clone)]
pub struct Method {
    rpc_manager: kademlia::RpcManager<SkyNode, KadHandler, 32, 20>,
    remote: Option<SkyNode>,
}

impl<'a> Method {
    pub fn from_manager(rpc_manager: &KadRpcManager, from: Option<SkyNode>) -> Self {
        Self {
            rpc_manager: rpc_manager.clone().into_inner(),
            remote: from,
        }
    }
}

impl rpc::Method for Method {
    type Req = Request;

    type Res = Response;

    type Error = Infallible;
}

impl rpc::Call for Method {
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

mod kad_handler;
mod kad_manager;
use kad_handler::KadHandler;
pub use kad_manager::KadRpcManager;
use rpc::traits::method::{can_transition, not_applicable::NotApplicable};

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

pub struct State;

impl rpc::traits::State for State {
    type ClientMethod = NotApplicable;

    type ServerMethod = Method;
}

#[derive(Clone)]
pub struct Method {
    rpc_manager: kademlia::RpcManager<SkyNode, KadHandler, 32, 20>,
    remote: Option<SkyNode>,
}

impl<'a> Method {
    pub fn new(rpc_manager: &KadRpcManager, from: Option<SkyNode>) -> Self {
        Self {
            rpc_manager: rpc_manager.clone().into_inner(),
            remote: from,
        }
    }
}

impl rpc::Method for Method {
    type Req = Request;

    type Res = Response;

    type CanTransition = can_transition::False;
}

impl rpc::Handler for Method {
    type Error = Infallible;

    #[tracing::instrument(skip(self, replier))]
    async fn handle<Replier: rpc::transport::ReplyHelper<Self>>(
        &mut self,
        replier: Replier,
        value: <Self as rpc::Method>::Req,
    ) -> Result<
        <Replier as rpc::transport::ReplyHelper<Self>>::Receipt<Self>,
        rpc::traits::HandleError<
            <Replier as rpc::transport::ReplyHelper<Self>>::Error,
            <Self as rpc::Handler<Self>>::Error,
        >,
    > {
        let sky_id: kademlia::Id<32> = value.sky_id.into();
        let out = self
            .rpc_manager
            .find_node(self.remote.clone(), &sky_id)
            .await;

        trace!(?out);
        replier.reply(out.into()).await
    }
}

use std::convert::Infallible;

type LoopbackState = state::Wrapper<State>;

use maxlen::MaxLen;
use rpc::traits::method::can_transition;
use rpc::traits::method::not_applicable::NotApplicable;
use rpc::traits::state;
use shared_schema::SkyNode;

use super::find_nodes::KadRpcManager;

use super::find_nodes;
#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Request {
    #[n(0)]
    Ping(#[n(0)] shared_schema::ping::Request),
    #[n(1)]
    FindNodes(#[n(0)] find_nodes::Request),
}

impl From<shared_schema::ping::Request> for Request {
    fn from(value: shared_schema::ping::Request) -> Self {
        Self::Ping(value)
    }
}

impl From<find_nodes::Request> for Request {
    fn from(value: find_nodes::Request) -> Self {
        Self::FindNodes(value)
    }
}

pub enum Response {
    Ping(LoopbackState),

    FindNodes(find_nodes::Response, LoopbackState),
}

impl From<Response> for LoopbackState {
    fn from(value: Response) -> Self {
        match value {
            Response::Ping(method_wrapper) => method_wrapper,
            Response::FindNodes(_find_nodes_response, method_wrapper) => method_wrapper,
        }
    }
}

pub struct State;

impl rpc::traits::State for State {
    type ClientMethod = NotApplicable;

    type ServerMethod = Method;
}

#[derive(Clone)]
pub struct Method {
    find_nodes: find_nodes::Method,
}

impl std::fmt::Debug for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Method").finish()
    }
}

impl Method {
    pub fn new(rpc_manager: &KadRpcManager, from: SkyNode) -> Self {
        Self {
            find_nodes: find_nodes::Method::new(rpc_manager, Some(from)),
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
        Ok(match value {
            Request::Ping(request) => {
                replier
                    .reply_with(&mut shared_schema::ping::Method, request, |_res| {
                        Response::Ping(Default::default())
                    })
                    .await?
            }
            Request::FindNodes(find_nodes_request) => {
                replier
                    .reply_with(&mut self.find_nodes, find_nodes_request, |r| {
                        Response::FindNodes(r, LoopbackState::default())
                    })
                    .await?
            }
        })
    }
}

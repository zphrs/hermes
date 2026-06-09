pub mod register;

use std::convert::Infallible;

type LoopbackMethod = MethodWrapper<Method>;

use maxlen::MaxLen;
use rpc::MethodWrapper;
use shared_schema::EarthNode;

use crate::api::{
    earth_root::register::OnlineNodes,
    find_nodes::{self, KadRpcManager},
};
#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Request {
    #[n(0)]
    Ping(#[n(0)] shared_schema::ping::Request),
    #[n(1)]
    FindNodes(#[n(0)] find_nodes::Request),
    #[n(2)]
    Register(#[n(0)] register::Request),
}

impl From<find_nodes::Request> for Request {
    fn from(value: find_nodes::Request) -> Self {
        Self::FindNodes(value)
    }
}

impl From<shared_schema::ping::Request> for Request {
    fn from(value: shared_schema::ping::Request) -> Self {
        Self::Ping(value)
    }
}

pub enum Response {
    Ping(LoopbackMethod),
    FindNodes(find_nodes::Response, LoopbackMethod),
    Register(register::Response),
}

impl From<Response> for LoopbackMethod {
    fn from(value: Response) -> Self {
        match value {
            Response::Ping(method_wrapper) => method_wrapper,
            Response::FindNodes(_find_nodes_response, method_wrapper) => method_wrapper,
            Response::Register(_response) => LoopbackMethod::new(),
        }
    }
}

#[derive(Clone)]
pub struct Method {
    find_nodes: find_nodes::Method,
    register: register::Method,
}

impl std::fmt::Debug for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Method").finish()
    }
}

impl Method {
    pub fn new(rpc_manager: &KadRpcManager, from: EarthNode, online_nodes: OnlineNodes) -> Self {
        Self {
            find_nodes: find_nodes::Method::new(rpc_manager, None),
            register: register::Method::from_online_nodes(online_nodes, from),
        }
    }
}

impl rpc::Method for Method {
    type Req = Request;

    type Res = Response;

    type Error = Infallible;
}

impl rpc::Call for Method {
    async fn call<T: futures_io::AsyncWrite + Unpin + Send + Sync, TransportError>(
        &mut self,
        replier: rpc::Replier<'_, T, Self>,
        value: Self::Req,
    ) -> Result<rpc::ReplyReceipt<Self::Res>, rpc::ClientError<TransportError, Self::Error>> {
        Ok(match value {
            Request::Ping(request) => replier
                .change_method(&request)
                .reply_with(&mut shared_schema::ping::Method, request)
                .await?
                .map(|_res| Response::Ping(Default::default())),
            Request::FindNodes(find_nodes_request) => replier
                .change_method(&find_nodes_request)
                .reply_with(&mut self.find_nodes, find_nodes_request)
                .await?
                .map(|r| Response::FindNodes(r, LoopbackMethod::default())),
            Request::Register(request) => replier
                .change_method(&request)
                .reply_with(&mut self.register, request)
                .await?
                .map(Response::Register),
        })
    }
}

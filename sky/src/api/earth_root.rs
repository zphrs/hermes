use std::convert::Infallible;

type LoopbackMethod = MethodWrapper<Method>;

use maxlen::MaxLen;
use rpc::MethodWrapper;
use shared_schema::SkyNode;

use crate::api::find_nodes_method::{FindNodesMethod, FindNodesResponse, KadRpcManager};

use super::find_nodes_method;
#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Request {
    #[n(0)]
    FindNodes(#[n(0)] find_nodes_method::FindNodesRequest),
}

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
#[cbor(flat)]
pub enum Response {
    #[n(0)]
    FindNodes(#[n(0)] FindNodesResponse, #[cbor(skip)] LoopbackMethod),
}

impl From<Response> for LoopbackMethod {
    fn from(value: Response) -> Self {
        match value {
            Response::FindNodes(_find_nodes_response, method_wrapper) => method_wrapper,
        }
    }
}

#[derive(Clone)]
pub struct Method {
    find_nodes: FindNodesMethod,
}

impl std::fmt::Debug for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Method").finish()
    }
}

impl Method {
    pub fn new(rpc_manager: &KadRpcManager, from: SkyNode) -> Self {
        Self {
            find_nodes: FindNodesMethod::from_manager(rpc_manager, Some(from)),
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
            Request::FindNodes(find_nodes_request) => replier
                .change_method(&find_nodes_request)
                .reply_with(&mut self.find_nodes, find_nodes_request)
                .await?
                .map(|r| Response::FindNodes(r, LoopbackMethod::default())),
        })
    }
}

pub mod register;

use std::convert::Infallible;

use maxlen::MaxLen;
use rpc::{
    method::{Ancestor, ancestors, is_leaf},
    traits::method::{can_transition, not_applicable::NotApplicable},
};
use shared_schema::{EarthNode, ping};

use crate::api::{
    earth_root::register::OnlineNodes,
    find_nodes::{self, KadRpcManager},
};
#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Request {
    #[n(0)]
    Ping(#[n(0)] shared_schema::ping::Req),
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

impl From<shared_schema::ping::Req> for Request {
    fn from(value: shared_schema::ping::Req) -> Self {
        Self::Ping(value)
    }
}

pub enum Response {
    Ping(shared_schema::ping::Res),
    FindNodes(find_nodes::Response),
    Register(register::Response),
}

#[derive(Clone)]
pub struct Method {
    find_nodes: find_nodes::Method,
    register: register::Method,
}

impl Ancestor<shared_schema::ping::Method> for Method {}

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

pub struct State;

impl rpc::traits::State for State {
    type ClientHandles = NotApplicable;

    type ServerHandles = Method;
}

impl rpc::Method for Method {
    type Req = Request;

    type Res = Response;

    type CanTransition = can_transition::False;

    type IsLeaf = is_leaf::False;
}

pub trait Ancestors:
    ancestors::Three<Method, ping::Method, find_nodes::Method> + register::Ancestors
{
}

impl<T: ancestors::Three<Method, ping::Method, find_nodes::Method> + register::Ancestors + ?Sized>
    Ancestors for T
{
}

impl<RM: Ancestors> rpc::Handler<RM> for Method {
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RM, Self>>(
        &mut self,
        replier: Replier,
        value: rpc::ReqOf<Self>,
    ) -> rpc::traits::HandlerResult<RM, Self, Replier, Self::Error> {
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
                        Response::FindNodes(r)
                    })
                    .await?
            }
            Request::Register(request) => {
                replier
                    .reply_with(&mut self.register, request, Response::Register)
                    .await?
            }
        })
    }
}

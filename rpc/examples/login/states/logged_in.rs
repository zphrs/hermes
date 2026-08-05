pub mod logout;
pub mod ping;

use std::convert::Infallible;

use rpc::{
    define_prioritized,
    method::{can_transition, is_leaf, not_applicable::NotApplicable},
    state::priority::server_wins,
};

pub struct LoggedIn;

impl rpc::State for LoggedIn {
    type ClientHandles = NotApplicable;

    type ServerHandles = ServerMethod;
}

define_prioritized!(LoggedIn, server_wins);

pub struct ServerMethod;

impl rpc::method::Ancestor<logout::Method> for ServerMethod {}
impl rpc::method::Ancestor<ping::Method> for ServerMethod {}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
#[cbor(flat)]
pub enum RootReq {
    #[n(0)]
    Logout(#[n(0)] <logout::Method as rpc::Method>::Req),
    #[n(1)]
    Ping(#[n(0)] <ping::Method as rpc::Method>::Req),
}

impl TryFrom<RootReq> for ping::Req {
    type Error = RootReq;

    fn try_from(value: RootReq) -> Result<Self, Self::Error> {
        match value {
            RootReq::Ping(req) => Ok(req),
            other => Err(other),
        }
    }
}

impl From<ping::Req> for RootReq {
    fn from(value: ping::Res) -> Self {
        RootReq::Ping(value)
    }
}

pub enum RootRes {
    Logout(<logout::Method as rpc::Method>::Res),
    Ping(<ping::Method as rpc::Method>::Res),
}

impl From<ping::Res> for RootRes {
    fn from(value: ping::Res) -> Self {
        RootRes::Ping(value)
    }
}

impl rpc::Method for ServerMethod {
    type Req = RootReq;

    type Res = RootRes;

    type CanTransition = can_transition::True;

    type IsLeaf = is_leaf::False;
}

impl rpc::Handler<Self> for ServerMethod {
    type Error = Infallible;

    async fn handle<Replier: rpc::transport::ReplyHelper<Self, Self>>(
        &mut self,
        replier: Replier,
        value: <Self as rpc::Method>::Req,
    ) -> Result<Replier::Receipt<Self>, rpc::traits::HandleError<Replier::Error, Self::Error>> {
        match value {
            RootReq::Logout(logout) => {
                replier
                    .reply_with(&mut logout::Method, logout, RootRes::Logout)
                    .await
            }
            RootReq::Ping(ping) => {
                replier
                    .reply_with(&mut ping::Method, ping, RootRes::Ping)
                    .await
            }
        }
    }
}

use rpc::{
    method::{Ancestor, FromDescendant, can_transition, is_leaf},
    state::Prioritized,
};

use crate::{Username, max_len_str::MaxLenStr};

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
pub struct Message {
    #[n(0)]
    pub from: Username,
    #[n(1)]
    pub body: MaxLenStr<1024>,
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "from {}: {}", self.from, self.body)
    }
}

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
pub enum Notification {
    #[n(0)]
    Join(#[n(0)] Username),
    #[n(1)]
    Left(#[n(0)] Username),
    #[n(2)]
    Mesg(#[n(0)] Message),
}

pub mod close;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
pub enum ToClientReq {
    #[n(0)]
    Notif(#[n(0)] <Notify as rpc::Method>::Req),
    #[n(1)]
    Close(#[n(0)] <close::Method as rpc::Method>::Req),
}

pub enum ToClientRes {
    Notif(<Notify as rpc::Method>::Res),
    Close(<close::Method as rpc::Method>::Res),
}

pub struct Notify;

impl rpc::Method for Notify {
    type Req = Notification;

    type Res = ();

    type CanTransition = can_transition::False;

    type IsLeaf = is_leaf::True;
}

pub struct ToClient;

impl Ancestor<Notify> for ToClient {}
impl Ancestor<close::Method> for ToClient {}

impl FromDescendant<close::Method> for ToClient {
    fn from_descendant_req(
        request: <close::Method as rpc::Method>::Req,
    ) -> <Self as rpc::Method>::Req {
        ToClientReq::Close(request)
    }

    fn from_descendant_res(
        result: <close::Method as rpc::Method>::Res,
    ) -> <Self as rpc::Method>::Res {
        ToClientRes::Close(result)
    }

    fn try_into_descendant_req(
        request: Self::Req,
    ) -> Result<<close::Method as rpc::Method>::Req, Self::Req> {
        match request {
            ToClientReq::Close(child) => Ok(child),
            other => Err(other),
        }
    }
}

impl FromDescendant<Notify> for ToClient {
    fn from_descendant_req(request: <Notify as rpc::Method>::Req) -> <Self as rpc::Method>::Req {
        ToClientReq::Notif(request)
    }

    fn from_descendant_res(result: <Notify as rpc::Method>::Res) -> <Self as rpc::Method>::Res {
        ToClientRes::Notif(result)
    }

    fn try_into_descendant_req(
        request: Self::Req,
    ) -> Result<<Notify as rpc::Method>::Req, Self::Req> {
        match request {
            ToClientReq::Notif(child) => Ok(child),
            other => Err(other),
        }
    }
}

impl rpc::Method for ToClient {
    type Req = ToClientReq;

    type Res = ToClientRes;

    type CanTransition = can_transition::True;

    type IsLeaf = is_leaf::False;
}

pub mod from_client;

pub struct InRoom;

impl rpc::State for InRoom {
    type ClientHandles = ToClient;

    type ServerHandles = from_client::Method;
}

impl Prioritized for InRoom {
    type Priority = u8;

    fn client_priority(request: &<Self::ClientHandles as rpc::Method>::Req) -> Self::Priority {
        use ToClientReq::*;
        match request {
            Notif(_) => 0,
            Close(_) => 2,
        }
    }

    fn server_priority(request: &<Self::ServerHandles as rpc::Method>::Req) -> Self::Priority {
        use from_client::Req::*;
        match request {
            Loopback(_) => 0,
            Close(_) => 1,
            Leave => 1,
        }
    }
}

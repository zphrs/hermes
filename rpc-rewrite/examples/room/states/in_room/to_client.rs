use super::super::Entrypoint;
use rpc_rewrite::{
    cursor::state,
    method::{self, Descendant, ReqOf, ResOf},
};

use super::{Notify, close};

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum Req {
    #[n(0)]
    Notify(#[n(0)] ReqOf<'static, Notify>),
    #[n(1)]
    Close(#[n(0)] ReqOf<'static, close::Method>),
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum Res {
    #[n(0)]
    Notify(#[n(0)] ResOf<'static, Notify>),
    #[n(1)]
    Close(#[n(0)] state::Wrapper<Entrypoint>),
}

impl state::Has<Entrypoint> for Res {
    fn try_extract_wrapper(self) -> Result<state::Wrapper<Entrypoint>, Self> {
        match self {
            Res::Notify(()) => Err(self),
            Res::Close(wrapper) => Ok(wrapper),
        }
    }
}

/// Root of everything the client handles in `InRoom`.
pub struct ToClient;

impl method::Method for ToClient {
    type Req<'buf> = Req;
    type Res<'buf> = Res;

    type Type = method::BranchCanTransition;
}

impl Descendant<ToClient> for Notify {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, ToClient> {
        Req::Notify(req)
    }
    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, ToClient> {
        Res::Notify(res)
    }
}

impl Descendant<ToClient> for close::Method {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, ToClient> {
        Req::Close(req)
    }
    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, ToClient> {
        Res::Close(res)
    }
}

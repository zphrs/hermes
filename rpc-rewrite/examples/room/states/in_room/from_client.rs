use super::super::Entrypoint;
use rpc_rewrite::{
    cursor::state,
    method::{self, Descendant, ReqOf, ResOf},
};

use super::close;

pub mod leave;
pub mod loopback;
pub mod post;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum Req {
    #[n(0)]
    Loopback(#[n(0)] ReqOf<'static, loopback::Method>),
    /// close room
    #[n(1)]
    Close(#[n(0)] ReqOf<'static, close::Method>),
    /// leave room
    #[n(2)]
    Leave(#[n(0)] ReqOf<'static, leave::Method>),
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum Res {
    #[n(0)]
    Loopback(#[n(0)] ResOf<'static, loopback::Method>),
    #[n(1)]
    Close(#[n(0)] state::Wrapper<Entrypoint>),
    #[n(2)]
    Leave(#[n(0)] state::Wrapper<Entrypoint>),
}

impl state::Has<Entrypoint> for Res {
    fn try_extract_wrapper(self) -> Result<state::Wrapper<Entrypoint>, Self> {
        match self {
            Res::Loopback(_) => Err(self),
            Res::Close(wrapper) | Res::Leave(wrapper) => Ok(wrapper),
        }
    }
}

pub struct Method;

impl method::Method for Method {
    type Req<'buf> = Req;
    type Res<'buf> = Res;

    type Type = method::BranchCanTransition;
}

impl Descendant<Method> for loopback::Method {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, Method> {
        Req::Loopback(req)
    }
    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, Method> {
        Res::Loopback(res)
    }
}

impl Descendant<Method> for close::Method {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, Method> {
        Req::Close(req)
    }
    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, Method> {
        Res::Close(res)
    }
}

impl Descendant<Method> for leave::Method {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, Method> {
        Req::Leave(req)
    }
    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, Method> {
        Res::Leave(res)
    }
}

impl Descendant<Method> for post::Method {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, Method> {
        Req::Loopback(<Self as Descendant<loopback::Method>>::req_to_parent(req))
    }
    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, Method> {
        Res::Loopback(<Self as Descendant<loopback::Method>>::res_to_parent(res))
    }
}

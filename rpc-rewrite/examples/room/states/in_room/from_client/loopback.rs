use rpc_rewrite::method::{self, Descendant, ReqOf, ResOf};

use super::post;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum Req {
    /// send a message
    #[n(0)]
    Post(#[n(0)] ReqOf<'static, post::Method>),
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum Res {
    #[n(0)]
    Post(#[n(0)] ResOf<'static, post::Method>),
}

pub struct Method;

impl method::Method for Method {
    type Req<'buf> = Req;
    type Res<'buf> = Res;

    type Type = method::Branch<rpc_rewrite::marker::Loopback>;
}

impl Descendant<Method> for post::Method {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, Method> {
        Req::Post(req)
    }
    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, Method> {
        Res::Post(res)
    }
}

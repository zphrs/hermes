//! A minimal state whose root method has one loopback leaf and one transition
//! leaf. The server never handles requests in this test, so no handlers are
//! implemented.

use crate::{
    cursor::state,
    marker::{self, CanTransition, NotApplicable},
    method::{Descendant, ReqOf, ResOf},
};

pub struct State;

impl state::State for State {
    type ClientBranchType = marker::Loopback;
    type ClientHandles = NotApplicable;

    type ServerBranchType = CanTransition;
    type ServerHandles = RootMethod;
}

impl state::Entrypoint for State {}

pub struct RootMethod;

impl crate::Method for RootMethod {
    type Req<'buf> = RootRequest<'buf>;
    type Res<'buf> = RootResponse<'buf>;

    type Type = crate::method::BranchCanTransition;
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum RootRequest<'buf> {
    #[n(0)]
    Loopback(#[n(0)] ReqOf<'buf, Loopback>),
    #[n(1)]
    Transition(#[n(0)] ReqOf<'buf, Transition>),
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum RootResponse<'buf> {
    #[n(0)]
    Loopback(#[n(0)] ResOf<'buf, Loopback>),
    #[n(1)]
    Transition(#[n(0)] ResOf<'buf, Transition>),
}

pub struct Loopback;

impl crate::Method for Loopback {
    type Req<'buf> = ();
    type Res<'buf> = ();

    type Type = crate::method::LeafLoopback;
}

pub struct Transition;

impl crate::Method for Transition {
    type Req<'buf> = ();
    type Res<'buf> = state::Wrapper<NotApplicable>;

    type Type = crate::method::LeafTransition;
}

impl Descendant<RootMethod> for Loopback {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, RootMethod> {
        RootRequest::Loopback(req)
    }

    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, RootMethod> {
        RootResponse::Loopback(res)
    }
}

impl Descendant<RootMethod> for Transition {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, RootMethod> {
        RootRequest::Transition(req)
    }

    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, RootMethod> {
        RootResponse::Transition(res)
    }
}

use std::time::Duration;

use crate::{
    cursor::{self, state},
    marker::{NotApplicable, not_applicable},
    method::{
        Descendant, LeafHandler, ReqOf, ResOf, TransitionLeafHandler, handler::can_transition,
    },
};

pub struct State;
impl cursor::State for State {
    type ClientBranchType = crate::marker::Loopback;
    type ClientHandles = NotApplicable;

    type ServerBranchType = crate::marker::CanTransition;
    type ServerHandles = RootMethod;
}

impl state::Entrypoint for State {}

#[derive(Clone, Copy)]
pub struct RootMethod;

impl can_transition::BranchHandler for RootMethod {
    type NextHandler = Self;

    async fn handle_possible_transition<
        'a,
        R: crate::method::Replier<Self>
            + crate::method::replier::loopback::Replier<Self>
            + crate::method::replier::transition::Replier<Self>,
    >(
        self,
        request: ReqOf<'a, Self>,
        replier: R,
    ) -> Result<(R::Receipt<ResOf<'a, Self>>, Option<Self::NextHandler>), R::Error> {
        let res = match request {
            RootRequest::Ping(request) => {
                (replier.reply_with_leaf(request, &mut Ping).await?, None)
            }
            RootRequest::Transition(request) => {
                let (out, _next_handler) =
                    replier.transition_with_leaf(request, Transition).await?;
                (out, Some(self))
            }
            RootRequest::Sleep(request) => {
                (replier.reply_with_leaf(request, &mut Sleep).await?, None)
            }
        };

        Ok(res)
    }
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum RootRequest<'buf> {
    #[n(0)]
    Ping(#[n(0)] &'buf minicbor::bytes::ByteSlice),
    #[n(1)]
    Transition(#[n(0)] ReqOf<'buf, Transition>),
    #[n(2)]
    Sleep(#[n(0)] ReqOf<'buf, Sleep>),
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum RootResponse<'buf> {
    #[n(0)]
    Ping(#[n(0)] &'buf minicbor::bytes::ByteSlice),
    #[n(1)]
    Transition(#[n(0)] ResOf<'buf, Transition>),
    #[n(2)]
    Sleep(#[n(0)] ResOf<'buf, Sleep>),
}

pub struct Ping;

impl crate::Method for Ping {
    type Req<'buf> = &'buf minicbor::bytes::ByteSlice;
    type Res<'buf> = &'buf minicbor::bytes::ByteSlice;

    type Type = crate::method::LeafLoopback;
}

impl LeafHandler for Ping {
    async fn handle<'a>(&mut self, request: ReqOf<'a, Self>) -> ResOf<'a, Self> {
        request
    }
}

pub struct Transition;

impl TransitionLeafHandler for Transition {
    type NextHandler = not_applicable::Handler;

    async fn handle_transition<'a>(
        self,
        (): ReqOf<'a, Self>,
        wrapper_credit: state::WrapperCredit<Self>,
    ) -> (ResOf<'a, Self>, Self::NextHandler) {
        (wrapper_credit.into(), not_applicable::Handler)
    }
}

impl crate::Method for Transition {
    type Req<'buf> = ();
    type Res<'buf> = state::Wrapper<NotApplicable>;

    type Type = crate::method::LeafTransition;
}

pub struct Sleep;

impl crate::Method for Sleep {
    type Req<'buf> = Duration;
    type Res<'buf> = ();

    type Type = crate::method::LeafLoopback;
}

impl LeafHandler for Sleep {
    async fn handle<'a>(&mut self, request: ReqOf<'a, Self>) -> ResOf<'a, Self> {
        tokio::time::sleep(request).await;
    }
}

impl Descendant<RootMethod> for Ping {
    fn req_to_parent<'buf>(
        req: crate::method::ReqOf<'buf, Self>,
    ) -> crate::method::ReqOf<'buf, RootMethod> {
        RootRequest::Ping(req)
    }

    fn res_to_parent<'buf>(
        res: crate::method::ResOf<'buf, Self>,
    ) -> crate::method::ResOf<'buf, RootMethod> {
        RootResponse::Ping(res)
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

impl Descendant<RootMethod> for Sleep {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, RootMethod> {
        RootRequest::Sleep(req)
    }

    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, RootMethod> {
        RootResponse::Sleep(res)
    }
}

impl crate::Method for RootMethod {
    type Req<'buf> = RootRequest<'buf>;
    type Res<'buf> = RootResponse<'buf>;

    type Type = crate::method::BranchCanTransition;
}

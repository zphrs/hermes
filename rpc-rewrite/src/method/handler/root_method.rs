use std::marker::PhantomData;

use crate::marker::BranchType;
use crate::method::handler::{self, TransitionBranchHandler, TransitionLeafHandler, loopback};
use crate::method::{Branch, BranchHandler, Loopback, Transition};

use crate::method::{
    self, Descendant, Method, ReqOf, ResOf,
    handler::{LeafHandler, replier},
};

pub struct RootMethod<M: method::Method, BT: BranchType>(PhantomData<(M, BT)>);

impl<M: method::Method, BT: BranchType> Method for RootMethod<M, BT> {
    type Req<'buf> = ReqOf<'buf, M>;

    type Res<'buf> = ResOf<'buf, M>;

    type Type = Branch<BT>;
}

impl<M: method::Method, BT: BranchType> Descendant<RootMethod<M, BT>> for M {
    fn req_to_parent<'buf>(req: ReqOf<'buf, RootMethod<M, BT>>) -> ReqOf<'buf, Self> {
        req
    }

    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, RootMethod<M, BT>> {
        res
    }
}

#[derive(Clone)]
pub struct RootHandler<Handler>(pub Handler);

impl<BT: BranchType, M: method::OfType<method::Branch<BT>>, Handler: BranchHandler<M>>
    BranchHandler<RootMethod<M, BT>> for RootHandler<Handler>
{
    async fn handle<'a, R: method::Replier<RootMethod<M, BT>>>(
        &mut self,
        request: ReqOf<'a, RootMethod<M, BT>>,
        replier: R,
    ) -> Result<R::Receipt<ResOf<'a, RootMethod<M, BT>>>, R::Error> {
        replier.reply_with_branch(request, &mut self.0).await
    }
}

impl<M: method::OfType<method::LeafLoopback>, Handler: LeafHandler<M>>
    loopback::BranchHandler<RootMethod<M, Loopback>> for RootHandler<Handler>
where
    for<'a> ResOf<'a, M>: minicbor::CborLen<()> + minicbor::Encode<()>,
{
    async fn handle_loopback<
        'a,
        R: replier::Replier<RootMethod<M, Loopback>>
            + replier::loopback::Replier<RootMethod<M, Loopback>>,
    >(
        &mut self,
        request: ReqOf<'a, RootMethod<M, Loopback>>,
        replier: R,
    ) -> Result<
        <R as handler::Replier<RootMethod<M, Loopback>>>::Receipt<
            ResOf<'a, RootMethod<M, Loopback>>,
        >,
        R::Error,
    > {
        replier.reply_with_leaf(request, &mut self.0).await
    }
}

impl<M: method::OfType<method::LeafTransition>, Handler: TransitionLeafHandler<M>>
    TransitionBranchHandler<RootMethod<M, Transition>> for RootHandler<Handler>
where
    for<'a> ResOf<'a, M>: minicbor::CborLen<()> + minicbor::Encode<()>,
{
    type NextHandler = RootHandler<Handler::NextHandler>;

    async fn handle_transition<'a, TR: replier::transition::Replier<RootMethod<M, Transition>>>(
        self,
        request: ReqOf<'a, RootMethod<M, Transition>>,
        replier: TR,
    ) -> super::transition::HandleTransitionResult<
        'a,
        Self::NextHandler,
        TR,
        RootMethod<M, Transition>,
    > {
        replier::transition::Replier::transition_with_leaf(replier, request, self.0)
            .await
            .map(|res| (res.0, RootHandler(res.1)))
    }
}

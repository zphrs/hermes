use std::marker::PhantomData;

use crate::traits::{
    handler::{TransitionBranchHandler, TransitionLeafHandler},
    markers::True,
    method::{
        self, Descendant, Method, ReqOf, ResOf,
        handler::{BranchHandler, LeafHandler, replier},
    },
};

pub struct RootMethod<M: method::Method>(PhantomData<M>);

impl<M: method::Method> Method for RootMethod<M> {
    type Req<'buf> = ReqOf<'buf, M>;

    type Res<'buf> = ResOf<'buf, M>;

    type Transitions = M::Transitions;

    type HasDescendants = True;
}

impl<M: method::Method> Descendant<RootMethod<M>> for M {
    fn req_to_parent<'buf>(req: ReqOf<'buf, RootMethod<M>>) -> ReqOf<'buf, Self> {
        req
    }

    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, RootMethod<M>> {
        res
    }
}

pub struct RootHandler<Handler>(pub Handler);

impl<M: method::Leaf + method::Loopback, Handler: LeafHandler<M>> BranchHandler<RootMethod<M>>
    for RootHandler<Handler>
where
    for<'a> ResOf<'a, M>: minicbor::CborLen<()> + minicbor::Encode<()>,
{
    async fn handle<'req, Replier: replier::Replier<RootMethod<M>>>(
        &mut self,
        request: ReqOf<'req, RootMethod<M>>,
        replier: Replier,
    ) -> Result<Replier::Receipt<ResOf<'req, RootMethod<M>>>, Replier::Error> {
        replier.reply_with_leaf(request, &mut self.0).await
    }
}

impl<M: method::Leaf + method::Transitions, Handler: TransitionLeafHandler<M>>
    TransitionBranchHandler<RootMethod<M>> for RootHandler<Handler>
where
    for<'a> ResOf<'a, M>: minicbor::CborLen<()> + minicbor::Encode<()>,
{
    type NextHandler = RootHandler<Handler::NextHandler>;

    async fn handle_transition<'a, TR: replier::transition::Replier<RootMethod<M>>>(
        self,
        request: ReqOf<'a, RootMethod<M>>,
        replier: TR,
    ) -> super::transition::HandleTransitionResult<'a, Self::NextHandler, TR, RootMethod<M>> {
        replier::transition::Replier::reply_with_leaf(replier, request, self.0)
            .await
            .map(|res| (res.0, RootHandler(res.1)))
    }
}

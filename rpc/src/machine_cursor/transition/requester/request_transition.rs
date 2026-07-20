use std::{
    marker::PhantomData,
    pin::{Pin, pin},
    task::ready,
};

use futures::{FutureExt as _, future::FusedFuture};

use crate::{
    CallerError,
    machine_cursor::transition::{
        requester::processor_sacrifice::ProcessorSacrifice,
        transition_request_method::{self, TransitionRequestMethod},
    },
    traits::method::can_transition,
    transport::{CallerExt, PendingQueryOwned},
};

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct TransitionReceipt<Res, Role: crate::state::Role, Caller: crate::transport::Caller>(
    Res,
    Role,
    Caller,
);

impl<Role: crate::state::Role, Caller: crate::transport::Caller>
    TransitionReceipt<(), Role, Caller>
{
    pub(crate) fn insert_result<Res>(self, result: Res) -> TransitionReceipt<Res, Role, Caller> {
        TransitionReceipt(result, self.1, self.2)
    }

    #[expect(private_bounds)]
    pub(crate) async fn into_parts(
        mut self,
        processor: impl ProcessorSacrifice,
    ) -> Result<(Role, Caller), CallerError<<Caller as crate::Caller>::Error>>
    where
        Caller: crate::transport::Client,
    {
        processor.sacrifice(&mut self.2).await?;
        Ok((self.1, self.2))
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<Res, Role: crate::state::Role, Caller: crate::transport::Caller>
    TransitionReceipt<Res, Role, Caller>
{
    pub fn extract_result(self) -> (Res, TransitionReceipt<(), Role, Caller>) {
        (self.0, TransitionReceipt((), self.1, self.2))
    }

    pub fn connection_mut(&mut self) -> &mut Caller {
        &mut self.2
    }

    pub(crate) fn into_connection(self) -> Caller {
        self.2
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct RequestTransition<
    RootReq,
    M: crate::Method,
    Role: crate::state::Role,
    Caller: crate::transport::Caller,
> {
    query_req: PendingQueryOwned<Caller, TransitionRequestMethod<M>, RootReq>,
    role: Role,
}

impl<RootReq, M: crate::Method, Role: crate::state::Role, Caller: crate::transport::Caller> Unpin
    for RequestTransition<RootReq, M, Role, Caller>
{
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<
    RootReq: From<M::Req> + crate::RpcMessage,
    M: crate::Method<CanTransition = can_transition::True>,
    Role: crate::state::Role,
    Caller: crate::transport::Caller,
> RequestTransition<RootReq, M, Role, Caller>
where
    M::Res: crate::RpcMessage,
{
    pub fn new(req: M::Req, role: Role, caller: Caller) -> Self
    where
        M::Res: crate::RpcMessage,
    {
        Self {
            query_req: caller.query_owned(req),
            role,
        }
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<RootReq, M: crate::Method, Role: crate::state::Role, Caller: crate::transport::Caller>
    RequestTransition<RootReq, M, Role, Caller>
{
    pub fn query_req(&self) -> &PendingQueryOwned<Caller, TransitionRequestMethod<M>, RootReq> {
        &self.query_req
    }
    pub fn into_inner(self) -> PendingQueryOwned<Caller, TransitionRequestMethod<M>, RootReq> {
        self.query_req
    }
}

impl<
    RootReq,
    M: crate::Method,
    Role: crate::state::Role,
    Caller: crate::transport::Caller + CallerExt,
> FusedFuture for RequestTransition<RootReq, M, Role, Caller>
where
    M::Res: crate::RpcMessage,
    RootReq: crate::RpcMessage + From<<M as crate::Method>::Req>,
{
    fn is_terminated(&self) -> bool {
        self.query_req.is_terminated()
    }
}

impl<
    RootReq,
    M: crate::Method,
    Role: crate::state::Role,
    Caller: crate::transport::Caller + CallerExt,
> Future for RequestTransition<RootReq, M, Role, Caller>
where
    M::Res: crate::RpcMessage,
    RootReq: crate::RpcMessage + From<<M as crate::Method>::Req>,
{
    type Output = Result<
        (
            RootReq,
            TransitionReceipt<transition_request_method::Res, Role, Caller>,
        ),
        crate::transport::ext::query_owned::Error<Caller::Error, Caller, RootReq>,
    >;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = &mut *self;
        this.query_req.root_req();
        let res = ready!((this.query_req).poll_unpin(cx))?;
        std::task::Poll::Ready(Ok((res.2, TransitionReceipt(res.0, this.role, res.1))))
    }
}

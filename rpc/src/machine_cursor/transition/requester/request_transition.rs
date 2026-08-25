use crate::{method::not_applicable, traits::state::StateTypeIdExt as _};
use std::{marker::PhantomData, pin::Pin, task::ready};

use futures::{FutureExt as _, future::FusedFuture};
use tracing::trace;

use crate::{
    CallerError,
    machine_cursor::transition::{
        requester::ToSacrifice,
        transition_request_method::{self, TransitionRequestMethod},
    },
    method::{FromDescendant, is_leaf},
    traits::method::can_transition,
    transport::{CallerExt, PendingQueryOwned, ext::PrivateCallerExt},
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

#[expect(private_bounds, reason = "for role")]
impl<Role: crate::state::Role, Caller: crate::transport::Caller>
    TransitionReceipt<(), Role, Caller>
{
    pub(crate) fn insert_result<Res>(self, result: Res) -> TransitionReceipt<Res, Role, Caller> {
        TransitionReceipt(result, self.1, self.2)
    }

    /// Notifies the remote that we've sacrificed only if our local method
    /// was not NotApplicable.
    pub(crate) async fn into_parts<OldState: crate::State>(
        mut self,
        processor: ToSacrifice,
    ) -> Result<(Role, Caller), CallerError<<Caller as crate::Caller>::Error>>
    where
        Caller: crate::transport::Client + PartialEq,
    {
        if OldState::local_handles_type_id::<Role>() != not_applicable::TYPE_ID {
            processor.sacrifice(&mut self.2).await?;
        } else {
            trace!("skipping sending sacrifice notification");
        }
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

    pub fn connection(&self) -> &Caller {
        &self.2
    }

    pub fn connection_mut(&mut self) -> &mut Caller {
        &mut self.2
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct StageZero<
    RootMethod: crate::Method,
    M: crate::Method,
    Role: crate::state::Role,
    Caller: crate::transport::Caller,
> {
    root_req: RootMethod::Req,
    role: Role,
    caller: Caller,
    _marker: PhantomData<M>,
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<
    RootMethod: crate::Method,
    M: crate::Method,
    Role: crate::state::Role,
    Caller: crate::transport::Caller,
> StageZero<RootMethod, M, Role, Caller>
{
    pub fn new(req: M::Req, role: Role, caller: Caller) -> Self
    where
        RootMethod: FromDescendant<M>,
    {
        Self {
            root_req: RootMethod::from_descendant_req(req),
            role,
            caller,
            _marker: PhantomData,
        }
    }

    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    pub fn root_req(&self) -> &RootMethod::Req {
        &self.root_req
    }

    pub fn into_parts(self) -> (RootMethod::Req, Role, Caller) {
        (self.root_req, self.role, self.caller)
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct StageOne<
    RootMethod: crate::Method,
    M: crate::Method,
    Role: crate::state::Role,
    Caller: crate::transport::Caller,
> {
    query_req: PendingQueryOwned<Caller, TransitionRequestMethod<M>, RootMethod::Req>,
    role: Role,
}

impl<
    RootMethod: crate::Method,
    M: crate::Method,
    Role: crate::state::Role,
    Caller: crate::transport::Caller,
> Unpin for StageOne<RootMethod, M, Role, Caller>
{
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<
    RootMethod: crate::method::FromDescendant<M>,
    M: crate::Method<CanTransition = can_transition::True>,
    Role: crate::state::Role,
    Caller: crate::transport::Caller,
> StageOne<RootMethod, M, Role, Caller>
where
    M::Res: crate::RpcMessage,
{
    pub async fn new(
        root_req: RootMethod::Req,
        role: Role,
        caller: Caller,
    ) -> Result<Self, Caller::Error>
    where
        M: crate::Method<IsLeaf = is_leaf::True>,
        RootMethod: FromDescendant<M>,
        RootMethod::Req: crate::RpcMessage,
    {
        Ok(Self {
            query_req: caller
                .query_owned_from_root::<TransitionRequestMethod<M>, RootMethod::Req>(root_req)
                .await?,
            role,
        })
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<
    RootMethod: crate::Method,
    M: crate::Method,
    Role: crate::state::Role,
    Caller: crate::transport::Caller,
> StageOne<RootMethod, M, Role, Caller>
{
    pub fn query_req(
        &self,
    ) -> &PendingQueryOwned<Caller, TransitionRequestMethod<M>, RootMethod::Req> {
        &self.query_req
    }
    pub fn into_inner(
        self,
    ) -> PendingQueryOwned<Caller, TransitionRequestMethod<M>, RootMethod::Req> {
        self.query_req
    }
}

impl<
    RootMethod,
    M: crate::Method,
    Role: crate::state::Role,
    Caller: crate::transport::Caller + CallerExt,
> FusedFuture for StageOne<RootMethod, M, Role, Caller>
where
    M::Res: crate::RpcMessage,
    RootMethod: crate::Method,
    RootMethod::Req: crate::RpcMessage,
{
    fn is_terminated(&self) -> bool {
        self.query_req.is_terminated()
    }
}

impl<
    RootMethod: crate::Method,
    M: crate::Method,
    Role: crate::state::Role,
    Caller: crate::transport::Caller + CallerExt,
> Future for StageOne<RootMethod, M, Role, Caller>
where
    M::Res: crate::RpcMessage,
    RootMethod::Req: crate::RpcMessage,
{
    type Output = Result<
        (
            RootMethod::Req,
            TransitionReceipt<transition_request_method::Res, Role, Caller>,
        ),
        crate::transport::ext::query_owned::Error<Caller::Error, Caller, RootMethod::Req>,
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

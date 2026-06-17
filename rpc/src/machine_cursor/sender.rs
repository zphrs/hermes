use futures::FutureExt;
use std::{
    pin::{Pin, pin},
    task::ready,
};

use crate::{
    CallerError,
    traits::{
        self,
        method::{self, CanTransition, Loopback},
        state::{self, ToQuery},
    },
    transport::{CallerExt, PendingQueryOwned},
};

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct TransitionReceipt<Res, Role: state::Role, Caller: crate::transport::Caller>(
    Res,
    Role,
    Caller,
);

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<Res, Role: state::Role, Caller: crate::transport::Caller>
    TransitionReceipt<Res, Role, Caller>
{
    pub fn extract_result(self) -> (Res, TransitionReceipt<(), Role, Caller>) {
        (self.0, TransitionReceipt((), self.1, self.2))
    }

    pub(crate) fn into_connection(self) -> Caller {
        self.2
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct Sender<Role, RootMethod, Caller>
where
    Role: state::Role,
    Caller: crate::transport::Caller,
    RootMethod: crate::Method,
{
    _root_method: method::Wrapper<RootMethod>,
    role: Role,
    caller: Caller,
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<Role, RootMethod, Caller> Sender<Role, RootMethod, Caller>
where
    Role: state::Role,
    Caller: crate::transport::Caller,
    RootMethod: crate::Method,
{
    pub fn into_parts(self) -> (Role, Caller) {
        (self.role, self.caller)
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct RequestTransition<
    RootReq,
    M: crate::Method,
    Role: traits::state::Role,
    Caller: crate::transport::Caller,
> {
    query_req: PendingQueryOwned<Caller, M, RootReq>,
    role: Role,
}

impl<RootReq, M: crate::Method, Role: traits::state::Role, Caller: crate::transport::Caller> Unpin
    for RequestTransition<RootReq, M, Role, Caller>
{
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<
    RootReq: From<M::Req> + crate::RpcMessage,
    M: crate::Method,
    Role: traits::state::Role,
    Caller: crate::transport::Caller,
> RequestTransition<RootReq, M, Role, Caller>
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

    pub fn query_req(&self) -> &PendingQueryOwned<Caller, M, RootReq> {
        &self.query_req
    }
}

impl<
    RootReq,
    M: crate::Method,
    Role: traits::state::Role + Unpin,
    Caller: crate::transport::Caller + Unpin + CallerExt,
> Future for RequestTransition<RootReq, M, Role, Caller>
where
    M::Res: Unpin + crate::RpcMessage,
    RootReq: crate::RpcMessage + From<<M as traits::method::Method>::Req>,
    CallerError<Caller::Error>: Unpin,
{
    type Output = Result<TransitionReceipt<M::Res, Role, Caller>, CallerError<Caller::Error>>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = &mut *self;
        let res = ready!(pin!(&mut this.query_req).poll_unpin(cx))?;
        std::task::Poll::Ready(Ok(TransitionReceipt(res.0, this.role, res.1)))
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<Role, RootMethod, Caller> Sender<Role, RootMethod, Caller>
where
    Role: state::Role,
    Caller: crate::transport::Caller,
    RootMethod: crate::Method,
{
    pub fn new(state_wrapper: &impl ToQuery<RootMethod, Role>, role: Role, caller: Caller) -> Self {
        Self {
            _root_method: state_wrapper.to_query(&role),
            role,
            caller,
        }
    }

    pub async fn request_loopback<M: Loopback>(
        &self,
        req: M::Req,
    ) -> Result<M::Res, CallerError<Caller::Error>>
    where
        RootMethod::Req: crate::RpcMessage + From<M::Req>,
        M::Res: crate::RpcMessage,
    {
        let res = self.caller.query::<M, RootMethod::Req>(req).await?;

        Ok(res)
    }

    pub fn request_transition<M: CanTransition>(
        self,
        req: M::Req,
    ) -> RequestTransition<RootMethod::Req, M, Role, Caller>
    where
        RootMethod::Req: crate::RpcMessage + From<M::Req>,
        M::Res: crate::RpcMessage,
    {
        let Self { role, caller, .. } = self;
        let out = RequestTransition::<_, M, _, _>::new(req, role, caller);

        out
    }

    pub fn from_parts(
        parts: TransitionReceipt<(), Role, Caller>,
        state_wrapper: &impl ToQuery<RootMethod, Role>,
    ) -> Self {
        Self::new(state_wrapper, parts.1, parts.2)
    }
}

use crate::{
    CallerError,
    traits::{
        method::{self, CanTransition, Loopback},
        state::{self, ToQuery},
    },
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

    pub async fn request_transition<M: CanTransition>(
        self,
        req: M::Req,
    ) -> Result<TransitionReceipt<M::Res, Role, Caller>, CallerError<Caller::Error>>
    where
        RootMethod::Req: crate::RpcMessage + From<M::Req>,
        M::Res: crate::RpcMessage,
    {
        let Self { role, caller, .. } = self;
        let res = caller.query::<M, RootMethod::Req>(req).await?;

        Ok(TransitionReceipt(res, role, caller))
    }

    pub fn from_parts(
        parts: TransitionReceipt<(), Role, Caller>,
        state_wrapper: &impl ToQuery<RootMethod, Role>,
    ) -> Self {
        Self::new(state_wrapper, parts.1, parts.2)
    }
}

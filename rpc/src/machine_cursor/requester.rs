use std::marker::PhantomData;

use crate::{
    CallerError,
    machine_cursor::transition::{RequestTransition, requester::RequesterTransition},
    traits::{
        method::{self, CanTransition, Loopback},
        state::{self, ToQuery},
    },
    transport::CallerExt,
};

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct Requester<State, Role, RootMethod, Caller>
where
    Role: state::Role,
    Caller: crate::transport::Caller,
    RootMethod: crate::Method,
{
    _root_method: method::Wrapper<RootMethod>,
    role: Role,
    caller: Caller,
    _marker: PhantomData<State>,
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<State, Role, RootMethod, Caller> Requester<State, Role, RootMethod, Caller>
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
impl<State: crate::traits::State, Role, RootMethod, Caller>
    Requester<State, Role, RootMethod, Caller>
where
    Role: state::Role,
    Caller: crate::transport::Caller,
    RootMethod: crate::Method,
{
    pub fn new(state_wrapper: &crate::state::Wrapper<State>, role: Role, caller: Caller) -> Self
    where
        crate::state::Wrapper<State>: ToQuery<RootMethod, Role>,
    {
        Self {
            _root_method: state_wrapper.to_query(&role),
            role,
            caller,
            _marker: PhantomData,
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
    ) -> RequesterTransition<State, RequestTransition<RootMethod::Req, M, Role, Caller>>
    where
        RootMethod::Req: crate::RpcMessage + From<M::Req>,
        M::Res: crate::RpcMessage,
    {
        let Self { role, caller, .. } = self;
        RequesterTransition::new(RequestTransition::<_, M, _, _>::new(req, role, caller))
    }
}

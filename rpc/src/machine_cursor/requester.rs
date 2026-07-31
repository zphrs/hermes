use std::marker::PhantomData;

use crate::{
    CallerError,
    machine_cursor::transition::{RequestTransition, requester::RequesterTransition},
    method::ancestor::Leaf,
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
    /// Specify the leaf method you are requesting as the generic parameter.
    ///
    /// For example, if the RootMethod has a Req type of:
    ///
    /// ```
    /// enum RootMethodRequest {
    ///     A(A::Req),
    ///     B(B::Req),
    /// }
    /// ```
    ///
    /// where A and B are structs that implement Method and you're calling
    /// method A, then do:
    ///
    /// ```
    /// request_transition::<A>(A::Req::new())
    /// ```
    ///
    /// Then the receiver should use `reply_with` in the RootMethod's Handler,
    /// conditionally calling A's or B's handler depending on the request type.
    ///
    /// It is recommended to only implement [RpcMessage](crate::RpcMessage)
    /// for the leaf response types. In the above example that would be `A::Res`
    /// and `B::Res`. This is to ensure that you don't accidentally have the
    /// client specify the RootMethod for the request_transition generic and the
    /// server reply with RootMethod::Res.
    ///
    /// The point of all of this is to minimize the number of bytes that need to
    /// be sent and parsed.
    pub fn request_transition<M: CanTransition>(
        self,
        req: M::Req,
    ) -> RequesterTransition<State, RequestTransition<RootMethod::Req, M, Role, Caller>>
    where
        RootMethod::Req: crate::RpcMessage + From<M::Req>,
        M::Res: crate::RpcMessage,
        M: Leaf<RootMethod>,
    {
        let Self { role, caller, .. } = self;
        RequesterTransition::new(RequestTransition::<_, M, _, _>::new(req, role, caller))
    }
}

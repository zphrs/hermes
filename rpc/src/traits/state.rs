mod has;
pub mod priority;
pub mod role;
pub use has::Has;

pub(crate) use priority::PrioritizedUnsafeExt;
pub use priority::{Prioritized, Priority};
pub(crate) use role::Role;

/// It's necessary to divide between what requests the client and the server can
/// perform as an entrypoint into establishing a symmetric state.
pub trait State {
    type ClientHandles: crate::Method;
    type ServerHandles: crate::Method;
}

pub type ServerReq<State> = <<State as self::State>::ServerHandles as crate::Method>::Req;
pub type ServerRes<State> = <<State as self::State>::ServerHandles as crate::Method>::Res;
pub type ClientReq<State> = <<State as self::State>::ClientHandles as crate::Method>::Req;
pub type ClientRes<State> = <<State as self::State>::ClientHandles as crate::Method>::Res;

use std::marker::PhantomData;

use maxlen::MaxLen;

use crate::traits::{self, method};

pub struct Wrapper<State: crate::traits::State> {
    _marker: PhantomData<State>,
}

impl<State: crate::traits::State> std::fmt::Debug for Wrapper<State> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("state::Wrapper")
            .field(&std::any::type_name::<State>())
            .finish()
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub trait ToQuery<Method: crate::traits::Method, Role: crate::traits::state::Role> {
    fn to_query(&self, role: &Role) -> method::Wrapper<Method>;
}

pub struct Handle<Method: crate::traits::Method, Handler: traits::Handler<Method, Method>> {
    _marker: method::Wrapper<Method>,
    handler: Handler,
}

impl<Method: crate::traits::Method, Handler: traits::Handler<Method, Method>>
    Handle<Method, Handler>
{
    pub(crate) fn into_parts(self) -> (method::Wrapper<Method>, Handler) {
        (self._marker, self.handler)
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub trait ToHandle<
    Method: crate::traits::Method,
    Role: crate::traits::state::Role,
    Handler: crate::traits::Handler<Method, Method>,
>
{
    fn to_handle(&self, role: &Role, handler: Handler) -> Handle<Method, Handler>;
}

impl<
    State: crate::traits::State,
    Handler: crate::traits::Handler<State::ServerHandles, State::ServerHandles>,
> ToHandle<State::ServerHandles, role::Server, Handler> for Wrapper<State>
where
    State::ServerHandles: method::Method,
{
    fn to_handle(
        &self,
        role: &role::Server,
        handler: Handler,
    ) -> Handle<State::ServerHandles, Handler> {
        let _ = role;
        Handle {
            _marker: method::Wrapper::new(),
            handler,
        }
    }
}

impl<
    State: crate::traits::State,
    Handler: crate::traits::Handler<State::ClientHandles, State::ClientHandles>,
> ToHandle<State::ClientHandles, role::Client, Handler> for Wrapper<State>
where
    State::ClientHandles: method::Method,
{
    fn to_handle(
        &self,
        role: &role::Client,
        handler: Handler,
    ) -> Handle<State::ClientHandles, Handler> {
        let _ = role;
        Handle {
            _marker: method::Wrapper::new(),
            handler,
        }
    }
}

impl<State: crate::traits::State> ToQuery<State::ServerHandles, role::Client> for Wrapper<State>
where
    State::ServerHandles: crate::Method,
{
    fn to_query(&self, role: &role::Client) -> method::Wrapper<State::ServerHandles> {
        let _ = role;
        method::Wrapper::new()
    }
}

impl<State: crate::traits::State> ToQuery<State::ClientHandles, role::Server> for Wrapper<State>
where
    State::ClientHandles: crate::Method,
{
    fn to_query(&self, role: &role::Server) -> method::Wrapper<State::ClientHandles> {
        let _ = role;
        method::Wrapper::new()
    }
}

impl<'b, C, State: crate::traits::State> minicbor::Decode<'b, C> for Wrapper<State> {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        Ok(Self {
            _marker: PhantomData::<State>::decode(d, ctx)?,
        })
    }
}

impl<State: crate::traits::State> MaxLen for Wrapper<State> {
    fn biggest_instantiation() -> Self {
        Self::new_without_check()
    }
}

impl<C, State: crate::traits::State> minicbor::CborLen<C> for Wrapper<State> {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        self._marker.cbor_len(ctx)
    }
}

impl<C, State: crate::traits::State> minicbor::Encode<C> for Wrapper<State> {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        self._marker.encode(e, ctx)
    }
}

impl<State: crate::traits::State> Wrapper<State> {
    pub(crate) fn duplicate(&self) -> Self {
        Self::new_without_check()
    }
    // creates a new wrapper without ensuring that it is
    // done only via the replier.
    pub(crate) fn new_without_check() -> Wrapper<State> {
        Self {
            _marker: PhantomData,
        }
    }
}

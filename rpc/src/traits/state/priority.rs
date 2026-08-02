//! Defines the [`Prioritized`] trait (alongside the [`Priority`] helper trait)
//! that is used to tiebreak between concurrent server and client state machine
//! transitions. See [`MachineCursor`] for where [`Prioritized`] is required.
//!
//! To better illustrate why a Priority is necessary, consider the following
//! execution where the possible states are A, B, and C, A is the entrypoint
//! state, and where the client can initiate a transition from A->B and from
//! B->C and the server can initiate a transition from B->A.
//!
//! ```text
//! C               S
//! |               |
//! |-T(B)--------->| # client requests transition from entrypoint A to B
//! |<----Ack(T(B))-| # server acks transition
//! |-L(B)--------->| # client says it's no longer handling requests for state A
//! |               |
//! |-T(C)-         | # client requests transition from B to C
//! |<-----\---T(A)-| # server requests transition from B->A
//! |       ------->|
//! |               | # choose(C.priority(), A.priority()) == Server (A wins)
//! |-Ack(T(A))---->| # Server discards the T(C) request, client acks T(A)
//! |<---------L(A)-| # server says it's no longer handling requests for state B
//! |               |
//! ```

use std::any::TypeId;

use crate::traits::State;

mod r#enum {
    #[repr(u8)]
    pub enum Priority {
        Client = 1,
        Server = 2,
    }
}

pub use r#enum::Priority::{Client, Server};

/// Defined on any type that can be used to tiebreak between transition requests
/// made concurrently by both the client and the server.
///
/// To avoid undefined behavior where the Client and the Server end up
/// out-of-sync with one another, any implementation MUST have [`choose`] be
/// deterministic.
///
/// # Comparison with [`PartialOrd`]/[`Ord`]
///
/// [`PartialOrd`] implies transitivity that is not necessary to tiebreak
/// between a client and a server request. See below for an implementation of
/// Priority for any type where `Self` is a [`PartialOrd`].
pub trait Priority<State: crate::traits::State>: Sized {
    /// Either returns [`Client`] or [`Server`], depending on which request
    /// should be kept and which should be discarded. Used to tiebreak between
    /// two transitioning requests that are pending simultaneously.
    ///
    /// # Default Implementation
    ///
    /// By default, prioritizes the server's request over the client's.
    #[expect(unused_variables)]
    fn choose(client: Self, server: Self) -> r#enum::Priority {
        Server
    }
}

/// Used to get the [`Priority`] of either a client request or a server request
/// to be stored during the querying or processing of the request.
///
/// To avoid undefined behavior where the Client and the Server end up
/// out-of-sync with one another, any implementation MUST have both the mapping
/// from a client or a server request to the associated
/// [`Priority`](Prioritized::Priority) be deterministic.
///
/// Super-trait of [`State`] because any [`State`] should only have one possible
/// way to prioritize between its associated [`Client`](State::ClientMethod) and
/// [`Server`](State::ServerMethod) methods.
pub trait Prioritized: State + Sized {
    /// The type used to prioritize one transition request over another.
    type Priority: Priority<Self>;
    /// Returns the [`Self::Priority`](Prioritized::Priority) of the client's
    /// transition request.
    fn client_priority(request: &<Self::ClientMethod as crate::Method>::Req) -> Self::Priority;
    /// Returns the [`Self::Priority`](Prioritized::Priority) of the server's
    /// transition request.
    fn server_priority(request: &<Self::ServerMethod as crate::Method>::Req) -> Self::Priority;
}

/// Dispatches to [`Prioritized::client_priority`] or [`Prioritized::server_priority`] based on
/// the caller's role and whether the context is a **Processor** or **Requester**.
///
/// Four methods form a 2×2 matrix:
///
/// - **Processor** roles map directly: client→client, server→server.
/// - **Requester** roles are swapped (e.g. server→client) because it owns both
/// sides but receives callbacks from the *other* side.
/// - `_static` variants assert [`TypeId`] equality (`'static` required).
/// - Non-`_static` variants assert [`size_of`] equality (no `'static` bound).
///
/// |                | `_static`                   | non-`_static`        |
/// |----------------|-----------------------------|----------------------|
/// | **Processor**  | `processor_priority_static` | `processor_priority` |
/// | **Requester**  | `requester_priority_static` | `requester_priority` |
///
/// The method body is identical between static and non-static variants; only the
/// assertion differs ([`assert_same_type_id`] vs [`assert_same_size`]).
pub(crate) trait PrioritizedUnsafeExt: Prioritized {
    /// Caller must ensure that this is only called within a Processor with the
    /// passed in RootMethod being the RootMethod of the Processor.
    #[allow(dead_code)]
    unsafe fn processor_priority_static<Role: crate::state::Role, RootRequest>(
        req: &RootRequest,
    ) -> Self::Priority
    where
        RootRequest: 'static,
        <Self::ServerMethod as crate::Method>::Req: 'static,
        <Self::ClientMethod as crate::Method>::Req: 'static,
    {
        match Role::to_enum() {
            crate::state::role::WhichRole::Client => {
                assert_same_type_id::<RootRequest, Self::ClientMethod>();
                Self::client_priority(unsafe { std::mem::transmute(req) })
            }
            crate::state::role::WhichRole::Server => {
                assert_same_type_id::<RootRequest, Self::ServerMethod>();
                Self::server_priority(unsafe { std::mem::transmute(req) })
            }
        }
    }
    /// Caller must ensure that this is only called within a Processor with the
    /// passed in RootMethod being the RootMethod of the Processor.
    unsafe fn processor_priority<Role: crate::state::Role, RootRequest>(
        req: &RootRequest,
    ) -> Self::Priority {
        match Role::to_enum() {
            crate::state::role::WhichRole::Client => {
                assert_same_size::<RootRequest, Self::ClientMethod>();
                Self::client_priority(unsafe { std::mem::transmute(req) })
            }
            crate::state::role::WhichRole::Server => {
                assert_same_size::<RootRequest, Self::ServerMethod>();
                Self::server_priority(unsafe { std::mem::transmute(req) })
            }
        }
    }
    /// Caller must ensure that this is only called within a Requester with the
    /// passed in RootMethod being the RootMethod of the Requester.
    #[allow(dead_code)]
    unsafe fn requester_priority_static<Role: crate::state::Role, RootRequest>(
        req: &RootRequest,
    ) -> Self::Priority
    where
        RootRequest: 'static,
        <Self::ServerMethod as crate::Method>::Req: 'static,
        <Self::ClientMethod as crate::Method>::Req: 'static,
    {
        match Role::to_enum() {
            crate::state::role::WhichRole::Server => {
                assert_same_type_id::<RootRequest, Self::ClientMethod>();
                Self::client_priority(unsafe { std::mem::transmute(req) })
            }
            crate::state::role::WhichRole::Client => {
                assert_same_type_id::<RootRequest, Self::ServerMethod>();
                Self::server_priority(unsafe { std::mem::transmute(req) })
            }
        }
    }
    /// Caller must ensure that this is only called within a Requester with the
    /// passed in RootMethod being the RootMethod of the Requester.
    unsafe fn requester_priority<Role: crate::state::Role, RootRequest>(
        req: &RootRequest,
    ) -> Self::Priority {
        match Role::to_enum() {
            crate::state::role::WhichRole::Server => {
                assert_same_size::<RootRequest, Self::ClientMethod>();
                Self::client_priority(unsafe { std::mem::transmute(req) })
            }
            crate::state::role::WhichRole::Client => {
                assert_same_size::<RootRequest, Self::ServerMethod>();
                Self::server_priority(unsafe { std::mem::transmute(req) })
            }
        }
    }
}

impl<T: Prioritized> PrioritizedUnsafeExt for T {}

fn assert_same_type_id<RootRequest, PMethod: crate::Method>()
where
    PMethod::Req: 'static,
    RootRequest: 'static,
{
    assert_eq!(TypeId::of::<RootRequest>(), TypeId::of::<PMethod::Req>());
}

fn assert_same_size<RootRequest, PMethod: crate::Method>() {
    assert_eq!(size_of::<RootRequest>(), size_of::<PMethod::Req>());
}

impl<S: crate::traits::State, T: PartialOrd> Priority<S> for T {
    /// Defines Priority as a super-trait of PartialOrd. This allows use of any
    /// type that implements [`PartialOrd`] as a [`Priority`], like the
    /// built in numeric primitives ([`u8`], [`i8`], [`u16`] etc.).
    fn choose(client: T, server: T) -> r#enum::Priority {
        if client > server { Client } else { Server }
    }
}

/// Priority type that compares by cloning the request values and comparing them.
/// Used by the `from_cloned_requests` strategy.
pub enum SelfPriority<S: State> {
    Client(<S::ClientMethod as crate::Method>::Req),
    Server(<S::ServerMethod as crate::Method>::Req),
}

impl<S: State> Priority<S> for SelfPriority<S>
where
    <S::ClientMethod as crate::Method>::Req: PartialOrd<<S::ServerMethod as crate::Method>::Req>,
{
    fn choose(client: Self, server: Self) -> r#enum::Priority {
        let Self::Client(client) = client else {
            return Server;
        };

        let Self::Server(server) = server else {
            return Server;
        };

        if client > server { Client } else { Server }
    }
}

/// Priority strategy that always gives precedence to the server's request.
///
/// Unlike [`from_cloned_requests`], this strategy imposes no trait requirements whatsoever:
/// request types need not implement [`Clone`] or [`PartialOrd`].
///
/// # Example
///
/// ```ignore
/// use rpc::{State, Method, define_prioritized};
/// use rpc::traits::state::priority::server_wins;
///
/// // Minimal method types for demonstration
/// pub struct ClientM;
/// impl Method for ClientM {
///     type Req = String;
///     type Resp = ();
/// }
///
/// pub struct ServerM;
/// impl Method for ServerM {
///     type Req = String;
///     type Resp = ();
/// }
///
/// pub struct MyState;
/// impl State for MyState {
///     type ClientMethod = ClientM;
///     type ServerMethod = ServerM;
/// }
///
/// // Server requests always win over client requests.
/// define_prioritized!(MyState, server_wins);
/// ```
pub mod server_wins {
    use std::marker::PhantomData;

    pub type Priority<S> = (bool, PhantomData<S>);

    pub fn client_priority<State: crate::State>(
        _request: &<State::ClientMethod as crate::Method>::Req,
    ) -> Priority<State> {
        (false, PhantomData)
    }

    pub fn server_priority<State: crate::State>(
        _request: &<State::ServerMethod as crate::Method>::Req,
    ) -> Priority<State> {
        (true, PhantomData)
    }
}

/// Priority strategy that compares priorities by cloning the request values.
///
/// This strategy uses [`SelfPriority`], which wraps either the client or server
/// request and compares them via [`PartialOrd`].
///
/// # State requirements
///
/// The state `S` must satisfy the constraints of [`SelfPriority`]:
///
/// - `<S::ClientMethod as Method>::Req: Clone`
/// - `<S::ServerMethod as Method>::Req: Clone`
/// - `<S::ClientMethod as Method>::Req: PartialOrd<<S::ServerMethod as Method>::Req>`
///
/// That is, the client request type must implement `PartialOrd` against the
/// server request type (`client > server`), as defined by the [`Priority`] impl
/// on [`SelfPriority`]. Both request types must also be [`Clone`] so the values
/// can be stored in the returned SelfPriority struct for potential future
/// `choose` calls.
///
/// # Example
///
/// ```ignore
/// use rpc::{State, Method, define_prioritized};
/// use rpc::traits::state::priority::from_cloned_requests;
///
/// // Minimal method types for demonstration
/// pub struct ClientM;
/// impl Method for ClientM {
///     type Req = u64; // u64: Clone + PartialOrd<u64>
///     type Resp = ();
/// }
///
/// pub struct ServerM;
/// impl Method for ServerM {
///     type Req = u64; // same inner type as client req
///     type Resp = ();
/// }
///
/// pub struct MyState;
/// impl State for MyState {
///     type ClientMethod = ClientM;
///     type ServerMethod = ServerM;
/// }
///
/// // Derive Prioritized via the from_cloned_requests strategy.
/// // Client requests with higher u64 values "win" over server requests.
/// // Ties default to the Server request.
/// define_prioritized!(MyState, from_cloned_requests);
/// ```
pub mod from_cloned_requests {
    use super::SelfPriority;

    pub type Priority<S> = SelfPriority<S>;

    pub fn client_priority<S>(request: &<S::ClientMethod as crate::Method>::Req) -> Priority<S>
    where
        S: crate::State,
        <S::ClientMethod as crate::Method>::Req: Clone,
        <S::ServerMethod as crate::Method>::Req: Clone,
        <S::ClientMethod as crate::Method>::Req:
            PartialOrd<<S::ServerMethod as crate::Method>::Req>,
    {
        SelfPriority::Client(request.clone())
    }

    pub fn server_priority<S>(request: &<S::ServerMethod as crate::Method>::Req) -> Priority<S>
    where
        S: crate::State,
        <S::ClientMethod as crate::Method>::Req: Clone,
        <S::ServerMethod as crate::Method>::Req: Clone,
        <S::ClientMethod as crate::Method>::Req:
            PartialOrd<<S::ServerMethod as crate::Method>::Req>,
    {
        SelfPriority::Server(request.clone())
    }
}

/// Generates a `Prioritized` implementation for a state type using a strategy module.
///
/// The strategy module must provide:
///
/// - `Priority` — an associated type implementing [`Priority`]
/// - `client_priority` — function returning the client's priority
/// - `server_priority` — function returning the server's priority
///
/// # Example
///
/// ```ignore
/// use my_crate::State;
///
/// pub struct MyState;
/// impl State for MyState {
///     type ClientMethod = MyClientMethod;
///     type ServerMethod = MyServerMethod;
/// }
///
/// // Use the server_wins strategy module
/// define_prioritized!(MyState, crate::traits::state::priority::server_wins);
///
/// // Or: use from_cloned_requests
/// define_prioritized!(MyState, crate::traits::state::priority::from_cloned_requests);
/// ```
#[macro_export]
macro_rules! define_prioritized {
    ($StateType:ty, $($StrategyModule:tt)+) => {
        impl $crate::traits::Prioritized for $StateType {
            type Priority = $($StrategyModule)+::Priority<Self>;

            fn client_priority(
                request: &<Self::ClientMethod as $crate::Method>::Req,
            ) -> Self::Priority {
                $($StrategyModule)+::client_priority(request)
            }

            fn server_priority(
                request: &<Self::ServerMethod as $crate::Method>::Req,
            ) -> Self::Priority {
                $($StrategyModule)+::server_priority(request)
            }
        }
    };
}

#[cfg(test)]
mod test {
    use crate::{State, method::not_applicable::NotApplicable};

    #[allow(dead_code)]
    struct Test;

    impl State for Test {
        type ClientMethod = NotApplicable;

        type ServerMethod = NotApplicable;
    }

    // define_prioritized!(Test, self::server_wins);
    define_prioritized!(Test, super::from_cloned_requests);
}

//! Defines the [`Prioritized`] trait (alongside the [`Priority`] helper trait)
//! that is used to tiebreak between concurrent server and client state machine
//! transitions. See [`MachineCursor`] for where [`Prioritized`] is required.
//!
//! To better illustrate why a Priority is necessary, consider the following
//! execution where the possible states are A, B, and C, A is the entrypoint
//! state, and where the client can initiate a transition from A->B and from
//! B->C and the server can initiate a transition from B->A.
//!
//! ```
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

use crate::traits::State;

mod r#enum {
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

// ===== Macro-generated Prioritized implementations =====

/// Generates a `Prioritized` implementation for a state type.
///
/// # Strategies
///
/// - `server_wins` — the server always wins tiebreaks. `Priority = bool`,
///   client gets `false`, server gets `true`.
/// - `from_cloned_requests` — clones both request values and compares them
///   (`Clone + PartialOrd` required). Uses `SelfPriority<StateType>` as the
///   priority type.
///
/// # Example
///
/// ```ignore
/// pub struct MyState;
/// impl State for MyState {
///     type ClientMethod = MyClientMethod;
///     type ServerMethod = MyServerMethod;
/// }
///
/// // Server always wins (default tiebreak behavior)
/// define_prioritized!(MyState, server_wins);
///
/// // Or: compare cloned request values
/// define_prioritized!(MyState, from_cloned_requests);
/// ```
#[macro_export]
macro_rules! define_prioritized {
    // ── server_wins strategy ──
    ($StateType:ty, server_wins) => {
        impl $crate::traits::Prioritized for $StateType {
            type Priority = bool;

            fn client_priority(
                _request: &<Self::ClientMethod as $crate::Method>::Req,
            ) -> Self::Priority {
                false
            }

            fn server_priority(
                _request: &<Self::ServerMethod as $crate::Method>::Req,
            ) -> Self::Priority {
                true
            }
        }
    };

    // ── from_cloned_requests strategy ──
    ($StateType:ty, from_cloned_requests) => {
        impl $crate::traits::Prioritized for $StateType
        where
            <Self::ClientMethod as $crate::Method>::Req:
                Clone + PartialOrd<<Self::ServerMethod as $crate::Method>::Req>,
            <Self::ServerMethod as $crate::Method>::Req: Clone,
        {
            type Priority = $crate::traits::state::priority::SelfPriority<Self>;

            fn client_priority(
                request: &<Self::ClientMethod as $crate::Method>::Req,
            ) -> Self::Priority {
                $crate::traits::state::priority::SelfPriority::Client(request.clone())
            }

            fn server_priority(
                request: &<Self::ServerMethod as $crate::Method>::Req,
            ) -> Self::Priority {
                $crate::traits::state::priority::SelfPriority::Server(request.clone())
            }
        }
    };
}

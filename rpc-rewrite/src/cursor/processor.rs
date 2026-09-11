pub mod buffers;
pub mod loopback;
pub mod processor_fut;
pub mod transition;

use crate::{io::Connection, method};
pub use buffers::Buffers;
pub use processor_fut::ProcessorFut;
use std::{fmt::Debug, marker::PhantomData};

/// rpc processing follows these steps:
/// 1. accept stream
/// 2. [`Read`] request
/// 3. handle request (currently is infallible)
/// 4. reply response
#[derive(thiserror::Error)]
pub enum Error<C: Connection, ReplierError, HandlerError> {
    #[error("could not accept stream")]
    Accept(#[source] C::AcceptError),
    #[error("could not read request")]
    Read(#[from] crate::io::read::Error<C::RecvStream>),
    #[error("handler error: {0}")]
    Handler(#[source] HandlerError),
    #[error("could not reply")]
    Replier(#[source] ReplierError),
}

impl<C: Connection, ReplierError: Debug, HandlerError: Debug> std::fmt::Debug
    for Error<C, ReplierError, HandlerError>
where
    C::AcceptError: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Accept(arg0) => f.debug_tuple("Accept").field(arg0).finish(),
            Self::Read(arg0) => f.debug_tuple("Read").field(arg0).finish(),
            Self::Handler(arg0) => f.debug_tuple("Handler").field(arg0).finish(),
            Self::Replier(arg0) => f.debug_tuple("Replier").field(arg0).finish(),
        }
    }
}

pub struct Processor<State, Role, RootMethod: method::Branch, C: Connection, Handler> {
    connection: C,
    handler: Handler,
    _marker: PhantomData<(State, Role, RootMethod)>,
}

impl<State, Role, RootMethod: method::Branch, C: Connection, Handler>
    Processor<State, Role, RootMethod, C, Handler>
{
    pub(crate) fn new(connection: C, handler: Handler) -> Self {
        Self {
            handler,
            connection,
            _marker: PhantomData,
        }
    }
}

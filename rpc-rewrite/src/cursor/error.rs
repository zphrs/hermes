use crate::{
    io::{read, write},
    traits::Connection,
};

/// rpc request follows these steps:
/// 1. open stream
/// 2. [`Write`] request
/// 3. [`Read`] response
#[derive(thiserror::Error)]
pub enum Request<C: Connection> {
    #[error("could not open stream")]
    Open(#[source] C::OpenError),
    #[error("could not write request")]
    Write(#[from] write::Error<C>),
    #[error("could not read response")]
    Read(#[from] read::Error<C>),
}

/// rpc processing follows these steps:
/// 1. accept stream
/// 2. [`Read`] request
/// 3. handle request (currently is infallible)
/// 4. [`Write`] response
#[derive(thiserror::Error)]
pub enum Process<C: Connection, HandlerError> {
    #[error("could not accept stream")]
    Accept(#[source] C::AcceptError),
    #[error("could not read request")]
    Read(#[from] read::Error<C>),
    #[error("handler error: {0}")]
    Handler(#[source] HandlerError),
    #[error("could not write response")]
    Write(#[from] write::Error<C>),
}

pub mod notification {

    use crate::traits::Connection;

    /// notification follows these steps:
    /// 1. open unidirectional stream
    /// 2. [`Write`] notification
    #[derive(thiserror::Error)]
    pub enum Send<C: Connection> {
        #[error("stream could not be opened")]
        Open(#[source] C::OpenUniError),
        #[error("could not write notification")]
        Write(#[from] Write<C>),
    }
    /// 1. accept unidirectional stream
    /// 2. [`Read`] notification
    #[derive(thiserror::Error)]
    pub enum Receive<C: Connection> {
        #[error("could not accept stream")]
        Accept(#[source] C::AcceptUniError),
        #[error("could not read notification")]
        Read(#[from] Read<C>),
    }
}

/// requesting an rpc transition follows these steps:
/// 1. [`Request`] to transition
/// 2. sacrifice processor (infallible)
/// 3. [`Send` notification](notify::Send) of our processor sacrifice
#[derive(thiserror::Error)]
pub enum RequestTransition<C: Connection> {
    #[error("requesting transition failed")]
    Request(#[from] Request<C>),
    #[error("notification of processor sacrifice failed")]
    Notify(#[from] notification::Send<C>),
}

/// handling a transition follows these steps:
/// 1. [`Process`] request
/// 2. [`Receive`](notify::Receive) processor sacrifice notification
#[derive(thiserror::Error)]
pub enum HandleTransition<C: Connection, HandlerError> {
    #[error("processing transition failed")]
    Process(#[from] Process<C, HandlerError>),
    #[error("receiving notification of processor sacrifice failed")]
    ReceiveNotification(#[from] notification::Receive<C>),
}

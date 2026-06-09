use std::fmt::Debug;

mod state_machine_transitions;
#[cfg(test)]
mod tests;
pub mod traits;
pub mod transport;
mod wrappers;
pub use traits::{Call, Method, RpcMessage};

pub use wrappers::{client_conn, server_conn};

pub use state_machine_transitions::{ConcurrentRequestHandler, MethodWrapper};

pub use transport::{MemoryTransport, in_memory_transport};

// pub use state_machine_transitions::RootHandlerWrapper;

pub use crate::transport::{Caller, CallerError, ClientError, Replier, ReplyReceipt, Transport};

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("futures io: {0}")]
    FuturesIo(#[from] futures_io::Error),
    #[error("minicbor: {0}")]
    MinicborIo(#[from] minicbor_io::Error),
    #[error("stream closed")]
    Closed,
}

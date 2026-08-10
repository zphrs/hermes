use std::fmt::Debug;

pub mod machine_cursor;
#[cfg(test)]
mod tests;
pub mod traits;
pub mod transport;
pub use machine_cursor::MachineCursor;
pub use traits::{Handler, Method, ReqOf, ResOf, RpcMessage, State, method, state};

pub use transport::{MemoryTransport, ReplyHelper, in_memory_transport};

// pub use state_machine_transitions::RootHandlerWrapper;

pub use crate::transport::{
    Caller, CallerError, HandleOneRequestError, ImmediateReplier, ReplyReceipt, Transport,
};

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("minicbor: {0}")]
    MinicborIo(#[from] minicbor_io::Error),
    #[error("stream closed")]
    Closed,
}

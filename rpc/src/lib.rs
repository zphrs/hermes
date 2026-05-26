use std::fmt::Debug;

pub mod in_memory_transport;
mod state_machine_transitions;
#[cfg(test)]
mod tests;
mod transport;

pub use in_memory_transport::MemoryTransport;
pub use state_machine_transitions::RootHandlerWrapper;

use futures_io::{AsyncRead, AsyncWrite};

use crate::transport::{ClientError, Replier, ReplyReceipt};

pub use crate::transport::Transport;
use minicbor::{Decode, Encode};

pub trait RpcMessage: Debug + for<'a> Decode<'a, ()> + Encode<()> + maxlen::MaxLen {}

impl<T> RpcMessage for T where T: Debug + for<'a> Decode<'a, ()> + Encode<()> + maxlen::MaxLen {}

pub trait Method {
    type Req: Send;
    type Res: RpcMessage + Send;
    /// used to abort a reply midway through handling a request
    type Error;
}

pub trait StreamMethod: Method {
    type Req: RpcMessage + Send;
    type Res: RpcMessage;
}

pub trait Call: Method {
    fn call(
        &mut self,
        value: Self::Req,
    ) -> impl Future<Output = Result<Self::Res, Self::Error>> + Send;
    fn reply<T: futures_io::AsyncWrite + Unpin + Sync + Send, TransportError, Error>(
        &mut self,
        replier: crate::Replier<'_, T>,
        request: Self::Req,
    ) -> impl Future<Output = Result<ReplyReceipt<()>, crate::ClientError<TransportError, Error>>> + Send
    where
        Self: Send,
        Error: From<Self::Error>,
        Self::Res: Sync,
    {
        async {
            let res = self
                .call(request)
                .await
                .map_err(|e| crate::ClientError::App(Error::from(e)))?;
            replier
                .reply::<_, _, Self>(res)
                .await
                .map(ReplyReceipt::clear)
        }
    }
}

pub trait IntoRoot<M: Method, Root> {
    fn into_root(&self, req: M::Req) -> Root;
}

impl<M: Method, Root, T: ?Sized> IntoRoot<M, Root> for T
where
    Root: From<M::Req>,
{
    fn into_root(&self, req: M::Req) -> Root {
        req.into()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("futures io: {0}")]
    FuturesIo(#[from] futures_io::Error),
    #[error("minicbor: {0}")]
    MinicborIo(#[from] minicbor_io::Error),
    #[error("stream closed")]
    Closed,
}

pub trait RootHandler<Root: RpcMessage>: Sized + Send {
    type Response;
    type Error: Send;

    fn handle<T: futures_io::AsyncWrite + Unpin + Sync + Send, TransportError: Send>(
        &mut self,
        root: Root,
        replier: Replier<T>,
    ) -> impl std::future::Future<
        Output = Result<ReplyReceipt<Self::Response>, ClientError<TransportError, Self::Error>>,
    > + Send;
}

pub trait StreamTypes {
    // the error type
    type Error: std::error::Error;

    type RecvStream: AsyncRead;

    type SendStream: AsyncWrite;
}

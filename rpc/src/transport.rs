mod caller;
mod conn;
mod replier;
mod streams;

pub use conn::Connection;

pub use caller::Caller;

use maxlen::MaxLen;
use minicbor::Encode;
use minicbor_io::AsyncWriter;

pub use replier::{Replier, ReplyReceipt};
pub use streams::BiStream;

use crate::{Method, RpcError};
#[derive(Debug, thiserror::Error)]
pub enum CallerError<T> {
    #[error("rpc: {0}")]
    Rpc(#[from] RpcError),
    #[error("transport: {0}")]
    Transport(T),
}

use std::fmt::Debug;
#[derive(Debug, thiserror::Error)]
pub enum ClientError<T, E> {
    #[error("rpc: {0}")]
    Rpc(RpcError),
    #[error("transport: {0}")]
    Transport(T),
    #[error("app: {0}")]
    App(#[from] E),
}

impl<T, E> ClientError<T, E> {
    pub fn from_caller(err: CallerError<T>) -> Self {
        match err {
            CallerError::Rpc(rpc_error) => Self::Rpc(rpc_error),
            CallerError::Transport(t) => Self::Transport(t),
        }
    }
}

pub trait Client: Send + Sync + BiStream {
    type Error: Send;
    fn accept_stream(
        &self,
    ) -> impl Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::Error>> + Send;

    fn handle_one_request<'a, Rh: crate::Call + 'a + std::marker::Send>(
        &'a self,
        stream: &mut (Self::SendStream, Self::RecvStream),
        handler: &mut Rh,
    ) -> impl Future<Output = Result<Rh::Res, ClientError<Self::Error, Rh::Error>>> + Send
    where
        Rh::Error: Send,
        <Self as BiStream>::SendStream: Sync,
        Rh::Req: Debug,
        <Rh as Method>::Req: crate::RpcMessage,
    {
        async move {
            let (write, read) = stream;
            let mut receiver = minicbor_io::AsyncReader::new(read);

            receiver.set_max_len(Rh::Req::max_len() as u32);

            let Some(root) = receiver
                .read::<Rh::Req>()
                .await
                .map_err(|e| ClientError::Rpc(RpcError::from(e)))?
            else {
                return Err(ClientError::Rpc(RpcError::Closed));
            };
            let mut sender = minicbor_io::AsyncWriter::new(write);
            let out = match handler.call(Replier::new(&mut sender), root).await {
                Ok(v) => v,
                Err(e) => return Err(e),
            };
            Ok(out.into_inner())
        }
    }

    fn reply<T: futures::AsyncWrite + Unpin + Send, TransportError>(
        sender: &mut AsyncWriter<T>,
        res: impl Encode<()>,
    ) -> impl Future<Output = Result<ReplyReceipt<()>, ClientError<TransportError, Self::Error>>>
    {
        async move {
            sender
                .write(res)
                .await
                .map(|_| ReplyReceipt(()))
                .map_err(|e| ClientError::Rpc(e.into()))
        }
    }
}

pub trait Transport {
    /// how to dial a server, e.x. a SocketAddr
    type Address;
    /// transport error
    type Error;
    /// associated Caller with this transport
    type Caller: Caller;
    /// get a caller
    fn connect(
        &self,
        to: &Self::Address,
    ) -> impl Future<Output = Result<Self::Caller, Self::Error>> + Send;
    /// associated Client type with this transport
    type Client: Client;
    type Incoming: Incoming;
    fn accept(&self) -> impl Future<Output = Result<Self::Incoming, Self::Error>> + Send;
}

pub trait Incoming {
    type Client: Client;
    type Error;
    fn accept(self) -> impl Future<Output = Result<Self::Client, Self::Error>>;
}

pub trait Close {
    fn close(self) -> impl std::future::Future<Output = ()> + Send;
}

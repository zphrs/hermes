mod bi_stream;
mod caller;
mod conn;
pub mod in_memory_transport;
pub use in_memory_transport::MemoryTransport;
mod replier;

pub use conn::Connection;

pub use caller::Caller;

use maxlen::MaxLen;
use minicbor_io::AsyncWriter;

pub use bi_stream::BiStream;
pub use replier::{ImmediateReplier, ReplyHelper, ReplyReceipt};

#[derive(Debug, thiserror::Error)]
pub enum CallerError<T> {
    #[error("transport: {0}")]
    Transport(T),
    #[error("io: {0}")]
    Io(std::io::Error),
    #[error("connection closed")]
    Closed,
    #[error("misc. minicbor_io error: {0}")]
    Minicbor(minicbor_io::Error),
}

use std::{fmt::Debug, io::ErrorKind};

use crate::traits::method;
#[derive(Debug, thiserror::Error)]
pub enum HandleOneRequestError<R, E> {
    #[error("replier: {0}")]
    Replier(R),
    #[error("app: {0}")]
    App(E),
}

impl<R, E> From<crate::traits::HandlerError<R, E>> for HandleOneRequestError<R, E> {
    fn from(value: crate::traits::HandlerError<R, E>) -> Self {
        match value {
            crate::traits::HandlerError::Replier(r) => Self::Replier(r),
            crate::traits::HandlerError::Handler(h) => Self::App(h),
        }
    }
}

pub trait Client: BiStream {
    type Error;
    fn accept_stream(
        &self,
    ) -> impl Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::Error>>;
    // uses lifetime here to make it obvious that self is not captured in the
    // returned future.
    fn handle_one_request<'a, Method: crate::Method, Rh: crate::Handler<Method>>(
        &self,
        stream: &'a mut (Self::SendStream, Self::RecvStream),
        handler: &'a mut Rh,
    ) -> impl Future<
        Output = Result<Method::Res, HandleOneRequestError<minicbor_io::Error, Rh::Error>>,
    > + 'a
    where
        Method::Req: crate::RpcMessage,
    {
        async move {
            let (write, read) = stream;
            let mut receiver = minicbor_io::AsyncReader::new(read);

            receiver.set_max_len(Method::Req::max_len() as u32);

            let Some(root) = (match receiver.read::<Method::Req>().await {
                Ok(v) => v,
                Err(e) => return Err(HandleOneRequestError::Replier(e)),
            }) else {
                return Err(HandleOneRequestError::Replier(minicbor_io::Error::Io(
                    ErrorKind::ConnectionAborted.into(),
                )));
            };
            let mut sender = minicbor_io::AsyncWriter::new(write);
            let out = match handler
                .handle(ImmediateReplier::new(&mut sender), root)
                .await
            {
                Ok(v) => v,
                Err(e) => return Err(e.into()),
            };
            Ok(out.into_inner())
        }
    }

    fn handle_one_notification<
        'a,
        Method: crate::Method<Res = method::not_applicable::NotApplicable>,
    >(
        &self,
        stream: &'a mut (Self::SendStream, Self::RecvStream),
    ) -> impl Future<Output = Result<Method::Req, minicbor_io::Error>> + 'a
    where
        Method::Req: crate::RpcMessage,
    {
        async move {
            let (_write, read) = stream;
            let mut receiver = minicbor_io::AsyncReader::new(read);

            receiver.set_max_len(Method::Req::max_len() as u32);

            let Some(root) = (match receiver.read::<Method::Req>().await {
                Ok(v) => v,
                Err(e) => return Err(e),
            }) else {
                return Err(minicbor_io::Error::Io(ErrorKind::ConnectionAborted.into()));
            };
            Ok(root)
        }
    }

    fn reply<T: futures::AsyncWrite + Unpin, TransportError, M: crate::Method>(
        sender: &mut AsyncWriter<T>,
        res: M::Res,
    ) -> impl Future<Output = Result<ReplyReceipt<M>, minicbor_io::Error>>
    where
        M::Res: minicbor::Encode<()>,
    {
        async move { sender.write(&res).await.map(|_| ReplyReceipt::new(res)) }
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
    ) -> impl Future<Output = Result<Self::Caller, Self::Error>>;
    /// associated Client type with this transport
    type Client: Client;
    type Incoming: Incoming;
    fn accept(&self) -> impl Future<Output = Result<Self::Incoming, Self::Error>>;
}

pub trait Incoming {
    type Client: Client;
    type Error;
    fn accept(self) -> impl Future<Output = Result<Self::Client, Self::Error>>;
}

pub trait Close {
    fn close(self) -> impl std::future::Future<Output = ()>;
}

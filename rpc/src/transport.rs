mod bi_stream;
mod caller;
mod conn;
pub mod in_memory_transport;
use futures::FutureExt as _;
pub use in_memory_transport::MemoryTransport;
mod replier;

pub use caller::ext::{PendingQuery, PendingQueryOwned};

pub use conn::Connection;

pub use caller::{Caller, CallerExt};

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
    #[error("call aborted")]
    Aborted,
}

use std::{fmt::Debug, io::ErrorKind};

use crate::traits::{self, method};
#[derive(Debug, thiserror::Error)]
pub enum HandleOneRequestError<R, E> {
    #[error("replier: {0}")]
    Replier(R),
    #[error("app: {0}")]
    App(E),
    #[error("read: {0}")]
    Read(#[from] minicbor_io::Error),
}

impl<R, E> From<crate::traits::HandleError<R, E>> for HandleOneRequestError<R, E> {
    fn from(value: crate::traits::HandleError<R, E>) -> Self {
        match value {
            crate::traits::HandleError::Replier(r) => Self::Replier(r),
            crate::traits::HandleError::Handler(h) => Self::App(h),
        }
    }
}

pub trait Client: BiStream {
    type Error;
    /// A future that does not capture `&self` (see [`Client::accept_stream`]) and is [`Unpin`].
    type AcceptStreamFut: ::core::future::Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::Error>>
        + Unpin;
    fn accept_stream(&self) -> Self::AcceptStreamFut;
    // uses an associated type to make it obvious that self is not captured in the
    // returned future.
    fn handle_one_request_with_handler<
        'a,
        Replier: ReplyHelper<Method> + 'a,
        Method: crate::Method,
        Rh: crate::Handler<Method>,
    >(
        &self,
        replier: Replier,
        stream: &'a mut Self::RecvStream,
        handler: &'a mut Rh,
    ) -> impl Future<
        Output = Result<
            Replier::Receipt<Method>,
            HandleOneRequestError<<Replier as ReplyHelper<Method>>::Error, Rh::Error>,
        >,
    > + 'a
    where
        Method::Req: crate::RpcMessage,
        <Self as BiStream>::SendStream: 'a,
        <Self as BiStream>::RecvStream: 'a,
    {
        async move {
            let write = replier;
            let read = stream;
            let mut receiver = minicbor_io::AsyncReader::new(read);
            receiver.set_max_len(Method::Req::max_len() as u32);
            let Some(root) = (match receiver.read::<Method::Req>().await {
                Ok(v) => v,
                Err(e) => Err(HandleOneRequestError::Read(e))?,
            }) else {
                return Err(HandleOneRequestError::Read(minicbor_io::Error::Io(
                    ErrorKind::ConnectionAborted.into(),
                )));
            };
            let out = match handler.handle(write, root).await {
                Ok(v) => v,
                Err(traits::HandleError::Handler(e)) => {
                    return Err(HandleOneRequestError::App(e));
                }
                Err(traits::HandleError::Replier(e)) => {
                    return Err(HandleOneRequestError::Replier(e));
                }
            };
            Ok(out)
        }
    }

    fn handle_one_request<'b, 'a, Method: crate::Method + 'a, Rh: crate::Handler<Method>>(
        &'b self,
        stream: &'a mut (Self::SendStream, Self::RecvStream),
        handler: &'a mut Rh,
    ) -> impl Future<
        Output = Result<
            Method::Res,
            HandleOneRequestError<
                <ImmediateReplier<Self::SendStream, Method> as ReplyHelper<Method>>::Error,
                Rh::Error,
            >,
        >,
    >
    where
        Method::Req: crate::RpcMessage,
        <Self as BiStream>::SendStream: 'a,
        <Self as BiStream>::RecvStream: 'a,
    {
        let (write, read) = stream;
        let replier = ImmediateReplier::from(write);
        let out = self
            .handle_one_request_with_handler(replier, read, handler)
            .map(|v| v.map(|v| v.into_inner()));
        out
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

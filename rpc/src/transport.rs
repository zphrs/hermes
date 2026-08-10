mod bi_stream;
mod caller;
mod conn;
pub mod in_memory_transport;
use futures::FutureExt as _;
pub use in_memory_transport::MemoryTransport;
mod replier;

pub use caller::ext::{self, PendingQuery, PendingQueryOwned};

pub use conn::Connection;

pub use caller::{Caller, CallerExt};

use maxlen::MaxLen;

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

impl<T, C, RootReq> TryFrom<query_owned::Error<T, C, RootReq>> for CallerError<T> {
    type Error = &'static str;

    fn try_from(value: query_owned::Error<T, C, RootReq>) -> Result<Self, Self::Error> {
        Ok(match value {
            query_owned::Error::Cancelled(_c, _root_req) => Err("query was cancelled")?,

            query_owned::Error::Minicbor(error) => CallerError::Minicbor(error),
            query_owned::Error::Transport(transport) => CallerError::Transport(transport),
            query_owned::Error::Closed => CallerError::Closed,
        })
    }
}

use std::{fmt::Debug, io::ErrorKind};

use crate::{
    traits::{self, method},
    transport::ext::query_owned,
};
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
}

pub(crate) trait ClientExt: Client {
    // uses an associated type to make it obvious that self is not captured in the
    // returned future.
    #[allow(unused)]
    fn handle_one_request_with_handler<
        'a,
        Replier: ReplyHelper<RootMethod, Method> + 'a,
        Method: crate::Method,
        RootMethod,
        Rh: crate::Handler<RootMethod, Method>,
    >(
        &self,
        replier: Replier,
        stream: &'a mut Self::RecvStream,
        handler: &'a mut Rh,
    ) -> impl Future<
        Output = Result<
            Replier::Receipt<Method>,
            HandleOneRequestError<<Replier as ReplyHelper<RootMethod, Method>>::Error, Rh::Error>,
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
    #[allow(unused)]
    fn handle_one_request<'b, 'a, Method: crate::Method + 'a, Rh: crate::Handler<RootMethod, Method>, RootMethod>(
        &'b self,
        stream: &'a mut (Self::SendStream, Self::RecvStream),
        handler: &'a mut Rh,
    ) -> impl Future<
        Output = Result<
            Method::Res,
            HandleOneRequestError<
                <ImmediateReplier<Self::SendStream, Method> as ReplyHelper<RootMethod, Method>>::Error,
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
            .handle_one_request_with_handler::<_, _, RootMethod, _>(replier, read, handler)
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
}

impl<T: Client> ClientExt for T {}

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

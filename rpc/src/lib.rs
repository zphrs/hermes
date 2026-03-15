use std::fmt::Debug;
#[cfg(test)]
mod example;

use futures_io::{AsyncRead, AsyncWrite};

use maxlen::MaxLen;
use minicbor::{Decode, Encode};
use minicbor_io::AsyncWriter;
use tracing::trace;

pub trait RpcMessage: Debug + for<'a> Decode<'a, ()> + Encode<()> + maxlen::MaxLen {}

impl<T> RpcMessage for T where T: Debug + for<'a> Decode<'a, ()> + Encode<()> + maxlen::MaxLen {}

pub trait Method {
    type Req: Send;
    type Res: RpcMessage + Send;
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
    ) -> impl Future<Output = Result<ReplyReceipt, crate::ClientError<TransportError, Error>>> + Send
    where
        Self: Send,
        Error: From<Self::Error>,
    {
        async {
            let res = self
                .call(request)
                .await
                .map_err(|e| crate::ClientError::App(Error::from(e)))?;
            replier.reply(res).await
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
        &mut self,
        to: Self::Address,
    ) -> impl Future<Output = Result<Self::Caller, Self::Error>>;
    /// associated Client type with this transport
    type Client: Client;
    fn accept(&mut self) -> impl Future<Output = Result<Self::Client, Self::Error>>;
}

pub trait BiStream {
    type RecvStream: AsyncRead + Unpin + Send + Sync;
    type SendStream: AsyncWrite + Unpin + Send + Sync;
}

pub trait Close {
    fn close(self) -> impl std::future::Future<Output = ()> + Send;
}

pub struct StreamPair<Bs: BiStream>(pub Bs::SendStream, pub Bs::RecvStream);
impl<Bs: BiStream> StreamPair<Bs> {
    fn write_mut(&mut self) -> &mut Bs::SendStream {
        &mut self.0
    }

    fn read_mut(&mut self) -> &mut Bs::RecvStream {
        &mut self.1
    }
}

impl<Bs: BiStream> From<(Bs::SendStream, Bs::RecvStream)> for StreamPair<Bs> {
    fn from(value: (Bs::SendStream, Bs::RecvStream)) -> Self {
        Self(value.0, value.1)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CallerError<T> {
    #[error("rpc: {0}")]
    Rpc(#[from] RpcError),
    #[error("transport: {0}")]
    Transport(T),
}

pub struct InitializedMessageStream<Bs: BiStream>(StreamPair<Bs>);

impl<Bs: BiStream> InitializedMessageStream<Bs> {
    pub(crate) fn new(stream_pair: StreamPair<Bs>) -> Self {
        Self(stream_pair)
    }

    fn write_mut(&mut self) -> &mut Bs::SendStream {
        self.0.write_mut()
    }

    fn read_mut(&mut self) -> &mut Bs::RecvStream {
        self.0.read_mut()
    }

    fn close(self) -> impl Future<Output = ()>
    where
        Bs::SendStream: Close,
    {
        async { self.0.0.close().await }
    }
}

pub trait Caller: Send + Sync + BiStream + Sized {
    type Error;
    fn open_stream(
        &mut self,
    ) -> impl Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::Error>> + Send;

    fn query<M: Method, Root: RpcMessage + From<M::Req> + Send>(
        &mut self,
        req: M::Req,
    ) -> impl Future<Output = Result<Option<M::Res>, CallerError<Self::Error>>> + Send {
        async {
            let (write, read) = self.open_stream().await.map_err(CallerError::Transport)?;

            let root = Root::from(req);
            let mut sender = minicbor_io::AsyncWriter::new(write);
            sender.write(root).await.map_err(RpcError::from)?;
            trace!("sent query");
            let mut receiver = minicbor_io::AsyncReader::new(read);
            receiver.set_max_len(<M::Res as MaxLen>::max_len() as u32);
            Ok(receiver.read::<M::Res>().await.map_err(RpcError::from)?)
        }
    }

    fn init_message_stream<M: StreamMethod, Root: RpcMessage + From<<M as Method>::Req> + Send>(
        &mut self,
        req: <M as Method>::Req,
    ) -> impl Future<Output = Result<InitializedMessageStream<Self>, CallerError<Self::Error>>> + Send
    {
        async {
            let (mut write, read) = self.open_stream().await.map_err(CallerError::Transport)?;

            let root = Root::from(req);
            let mut sender = minicbor_io::AsyncWriter::new(&mut write);
            sender.write(root).await.map_err(RpcError::from)?;
            Ok(InitializedMessageStream::new((write, read).into()))
        }
    }
    // just dropping it should close the stream
    fn close_message_stream<M: StreamMethod>(
        &mut self,
        msg_stream: InitializedMessageStream<Self>,
    ) -> impl Future<Output = ()> + Send
    where
        Self::SendStream: Close,
    {
        async { msg_stream.close().await }
    }

    fn send_to_message_stream<M: StreamMethod>(
        &mut self,
        req: <M as StreamMethod>::Req,
        msg_stream: &mut InitializedMessageStream<Self>,
    ) -> impl Future<Output = Result<(), CallerError<Self::Error>>> + Send {
        async {
            let mut sender = minicbor_io::AsyncWriter::new(msg_stream.write_mut());
            sender.write(req).await.map_err(RpcError::from)?;
            Ok(())
        }
    }

    fn query_from_stream<M: StreamMethod>(
        &mut self,
        streams: &mut InitializedMessageStream<Self>,
    ) -> impl Future<Output = Result<Option<<M as StreamMethod>::Res>, CallerError<Self::Error>>> + Send
    {
        async {
            let mut receiver = minicbor_io::AsyncReader::new(streams.read_mut());
            Ok(receiver
                .read::<<M as StreamMethod>::Res>()
                .await
                .map_err(RpcError::from)?)
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum ClientError<T, E> {
    #[error("rpc: {0}")]
    Rpc(RpcError),
    #[error("transport: {0}")]
    Transport(T),
    #[error("app: {0}")]
    App(#[from] E),
}

pub trait Client: Send + Sync + BiStream {
    type Error: Send;
    fn accept_stream(
        &mut self,
    ) -> impl Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::Error>> + Send;
    fn handle_one_request<'a, Root: RpcMessage, Rh: RootHandler<Root> + 'a>(
        &'a mut self,
        handler: &mut Rh,
    ) -> impl Future<Output = Result<(), ClientError<Self::Error, Rh::Error>>> + Send
    where
        Rh::Error: Send,
        <Self as BiStream>::SendStream: Sync,
    {
        async move {
            let (write, read) = self.accept_stream().await.map_err(ClientError::Transport)?;
            let mut receiver = minicbor_io::AsyncReader::new(read);
            let Some(root) = receiver
                .read::<Root>()
                .await
                .map_err(|e| ClientError::Rpc(RpcError::from(e)))?
            else {
                return Ok(());
            };
            let mut sender = minicbor_io::AsyncWriter::new(write);
            match handler
                .handle::<_, Self::Error>(root, Replier::new(&mut sender))
                .await
            {
                Ok(_) => {}
                Err(e) => return Err(e),
            }
            Ok(())
        }
    }

    fn handle_client<'a, Root: RpcMessage, Rh: RootHandler<Root> + 'a>(
        &'a mut self,
        mut handler: Rh,
    ) -> impl Future<Output = Result<(), ClientError<Self::Error, Rh::Error>>> + Send
    where
        Rh::Error: Send,
        <Self as BiStream>::SendStream: Sync,
    {
        async move {
            loop {
                self.handle_one_request::<Root, Rh>(&mut handler).await?;
            }
        }
    }

    fn reply<T: AsyncWrite + Unpin + Send, TransportError>(
        sender: &mut AsyncWriter<T>,
        res: impl Encode<()>,
    ) -> impl Future<Output = Result<ReplyReceipt, crate::ClientError<TransportError, Self::Error>>>
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

pub struct ReplyReceipt(());

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("futures io: {0}")]
    FuturesIo(#[from] futures_io::Error),
    #[error("minicbor: {0}")]
    MinicborIo(#[from] minicbor_io::Error),
}

pub struct Replier<'a, T: AsyncWrite + Unpin + Send + Sync> {
    client: &'a mut minicbor_io::AsyncWriter<T>,
}

impl<'a, T: AsyncWrite + Unpin + Send + Sync> Replier<'a, T> {
    pub(crate) fn new(client: &'a mut minicbor_io::AsyncWriter<T>) -> Self {
        Self { client }
    }
    pub fn reply<TransportError, Error>(
        self,
        res: impl Encode<()> + Send,
    ) -> impl Future<Output = Result<ReplyReceipt, crate::ClientError<TransportError, Error>>> + Send
    {
        async {
            let res = self.client.write(res).await;
            res.map(|_| ReplyReceipt(()))
                .map_err(|e| ClientError::Rpc(RpcError::from(e)))
        }
    }

    pub fn reply_with<TransportError: Send, Error, M: Call + Method>(
        self,
        handler: &mut M,
        req: M::Req,
    ) -> impl Future<Output = Result<ReplyReceipt, crate::ClientError<TransportError, Error>>>
    where
        <M as Method>::Res: Send,
        Error: From<M::Error>,
    {
        async {
            self.reply(
                handler
                    .call(req)
                    .await
                    .map_err(|e| crate::ClientError::App(Error::from(e)))?,
            )
            .await
        }
    }
}

pub trait RootHandler<Root: RpcMessage>: Sized + Send {
    type Error: Send;

    fn handle<T: futures_io::AsyncWrite + Unpin + Sync + Send, TransportError: Send>(
        &mut self,
        root: Root,
        replier: Replier<T>,
    ) -> impl std::future::Future<
        Output = Result<ReplyReceipt, ClientError<TransportError, Self::Error>>,
    > + Send;
}

pub trait StreamTypes {
    // the error type
    type Error: std::error::Error;

    type RecvStream: AsyncRead;

    type SendStream: AsyncWrite;
}

#[cfg(test)]
mod tests {
    use maxlen::MaxLen;

    use crate::{Call as _, RpcError};
    use std::convert::Infallible;

    use crate::ReplyReceipt;

    #[derive(
        Debug,
        minicbor_derive::Encode,
        minicbor_derive::Decode,
        minicbor_derive::CborLen,
        maxlen::MaxLen,
    )]
    #[cbor(flat)]
    pub enum Root {
        #[n(0)]
        Ping(#[n(0)] ping::Request),
        #[n(1)]
        Other(#[n(0)] other_ping::Request),
    }

    mod ping {
        use maxlen::MaxLen;
        use std::convert::Infallible;

        #[derive(
            Debug,
            minicbor_derive::Encode,
            minicbor_derive::Decode,
            minicbor_derive::CborLen,
            maxlen::MaxLen,
        )]
        #[allow(dead_code)]
        pub struct Request;

        impl From<Request> for super::Root {
            fn from(value: Request) -> Self {
                Self::Ping(value)
            }
        }

        #[derive(
            Debug,
            minicbor_derive::Encode,
            minicbor_derive::Decode,
            minicbor_derive::CborLen,
            maxlen::MaxLen,
        )]
        pub struct Response;

        pub struct Method;

        impl crate::Method for Method {
            type Req = Request;
            type Res = Response;
            type Error = Infallible;
        }

        impl crate::Call for Method {
            async fn call(&mut self, _value: Self::Req) -> Result<Self::Res, Self::Error> {
                Ok(Response)
            }
        }
    }

    mod other_ping {
        use maxlen::MaxLen;

        #[derive(
            Debug,
            minicbor_derive::Encode,
            minicbor_derive::Decode,
            minicbor_derive::CborLen,
            maxlen::MaxLen,
        )]
        pub struct Request;

        #[derive(
            Debug,
            minicbor_derive::Encode,
            minicbor_derive::Decode,
            minicbor_derive::CborLen,
            maxlen::MaxLen,
        )]
        pub struct Response;
    }

    #[allow(dead_code)]
    struct RootHandler;
    #[allow(dead_code)]
    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("rpc: {0}")]
        Rpc(#[from] RpcError),
    }

    impl crate::RootHandler<Root> for RootHandler {
        type Error = Infallible;

        async fn handle<T: futures_io::AsyncWrite + Unpin + Sync + Send, TransportError>(
            &mut self,
            root: Root,
            replier: crate::Replier<'_, T>,
        ) -> Result<ReplyReceipt, crate::ClientError<TransportError, Self::Error>> {
            match root {
                Root::Ping(request) => replier.reply(ping::Method.call(request).await?).await,
                Root::Other(_request) => replier.reply(other_ping::Response).await,
            }
        }
    }
    #[test]
    pub fn test() {
        // let tp;
    }
}

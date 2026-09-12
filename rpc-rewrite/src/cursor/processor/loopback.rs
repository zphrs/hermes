//! Processor implementation for when the root method cannot transition to any other states.

use super::Processor;
use crate::io::Connection;

use crate::{
    io::read::read,
    method::{self, ReqOf, ResOf, handler, replier, replier::Receipt},
};
use futures::{FutureExt, StreamExt, select, stream::FuturesUnordered};
use std::convert::Infallible;
use std::fmt::Debug;
use std::future::Future;
use std::pin::pin;

#[derive(thiserror::Error)]
pub enum Error<C: Connection, HandlerError> {
    #[error("could not accept stream")]
    Accept(#[source] C::AcceptError),
    #[error("could not read request")]
    Read(#[from] crate::io::read::Error<C::RecvStream>),
    #[error("handler request: {0}")]
    Handler(#[source] HandlerError),
    #[error("could not write response")]
    Write(#[from] crate::io::write::Error<C::SendStream>),
}

impl<C: Connection, HandlerError> std::fmt::Debug for Error<C, HandlerError>
where
    C::AcceptError: Debug,
    HandlerError: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Accept(arg0) => f.debug_tuple("Accept").field(arg0).finish(),
            Self::Read(arg0) => f.debug_tuple("Read").field(arg0).finish(),
            Self::Handler(arg0) => f.debug_tuple("Handler").field(arg0).finish(),
            Self::Write(arg0) => f.debug_tuple("Write").field(arg0).finish(),
        }
    }
}

impl<
    State,
    Role,
    RootMethod: method::OfType<method::Branch<method::Loopback>>,
    C: Connection,
    Handler: handler::loopback::BranchHandler<RootMethod>,
> Processor<State, Role, RootMethod, C, Handler>
{
    pub async fn handle_loopback_request<'buf>(
        &mut self,
        write: &'buf mut Vec<u8>,
    ) -> Result<ResOf<'buf, RootMethod>, Error<C, Infallible>>
    where
        ReqOf<'buf, RootMethod>: minicbor::Decode<'buf, ()>,
    {
        let stream = self
            .connection
            .accept_stream()
            .await
            .map_err(Error::Accept)?;

        let (send, recv) = stream;

        let replier = replier::immediate::Replier::new(send);
        let handler: &mut Handler = &mut self.handler;

        write.clear();
        let request: ReqOf<RootMethod> = read(write, recv).await?;
        let receipt = handler.handle_loopback(request, replier).await?;

        match receipt.finalize().await {
            Ok(res) => Ok(res),
            Err(e) => match e {},
        }
    }

    async fn handle_loopback_stream(
        stream: (C::SendStream, C::RecvStream),
        mut handler: Handler,
    ) -> Result<(), Error<C, Infallible>>
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
    {
        let (send, recv) = stream;
        let replier = replier::immediate::Replier::new(send);
        let mut buffer = Vec::new();

        buffer.clear();
        let request: ReqOf<RootMethod> = read(&mut buffer, recv).await?;
        let receipt = handler.handle_loopback(request, replier).await?;

        match receipt.finalize().await {
            Ok(_) => Ok(()),
            Err(e) => match e {},
        }
    }

    async fn handle_loopback_requests_inner(self) -> Error<C, Infallible>
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
        Handler: Clone,
    {
        let mut js = FuturesUnordered::new();
        let mut stream_fut = pin!(self.connection.accept_stream().fuse());

        loop {
            select! {
                stream = &mut stream_fut => {
                    match stream {
                        Ok(stream) => {
                            let handler = self.handler.clone();
                            stream_fut.set(self.connection.accept_stream().fuse());
                            js.push(Self::handle_loopback_stream(stream, handler));
                        }
                        Err(e) => return Error::Accept(e),
                    }
                }
                res = js.select_next_some() => {
                    if let Err(e) = res {
                        return e;
                    }
                }
            }
        }
    }

    pub fn handle_loopback_requests<'buffer>(
        self,
    ) -> super::ProcessorFut<impl Future<Output = Error<C, Infallible>>>
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
        Handler: Clone,
    {
        super::ProcessorFut::new(self.handle_loopback_requests_inner())
    }
}

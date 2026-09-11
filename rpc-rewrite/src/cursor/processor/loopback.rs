//! Processor implementation for when the root method cannot transition to any other states.

use super::Processor;
use crate::io::Connection;

use crate::{
    io::read::read,
    method::{self, ReqOf, ResOf, handler, replier, replier::Receipt},
};
use std::convert::Infallible;
use std::fmt::Debug;

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
    RootMethod: method::Branch + method::Loopback,
    C: Connection,
    Handler: handler::BranchHandler<RootMethod>,
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
        let receipt = handler.handle(request, replier).await?;

        match receipt.finalize().await {
            Ok(res) => Ok(res),
            Err(e) => match e {},
        }
    }

    async fn handle_loopback_requests_inner<'buffer>(mut self) -> Error<C, Infallible>
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
    {
        let mut buffer = Vec::new();
        loop {
            if let Err(e) = self.handle_loopback_request(&mut buffer).await {
                return e;
            }
        }
    }

    pub fn handle_loopback_requests<'buffer>(
        self,
    ) -> super::ProcessorFut<impl Future<Output = Error<C, Infallible>>>
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
    {
        super::ProcessorFut::new(self.handle_loopback_requests_inner())
    }
}

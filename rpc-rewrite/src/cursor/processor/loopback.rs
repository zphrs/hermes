//! Processor implementation for when the root method cannot transition to any other states.

use super::Processor;

use crate::traits::{
    self, Receipt as _, handler,
    io::BytesReadStream,
    method::{self, ReqOf, ResOf},
    replier,
};
use std::convert::Infallible;

#[derive(Debug, thiserror::Error)]
pub enum HandleLoopbackError<C: traits::Connection, Replier> {
    #[error("accept: {0}")]
    Accept(C::AcceptError),
    #[error("io: {0}")]
    Respond(#[from] crate::io::respond::Error<Replier, <C::RecvStream as BytesReadStream>::Error>),
}

impl<C: traits::Connection, Replier> From<Infallible> for HandleLoopbackError<C, Replier> {
    fn from(value: Infallible) -> Self {
        // we match to prove that `value` cannot be constructed
        match value {}
    }
}

impl<
    State,
    Role,
    RootMethod: method::Branch + method::Loopback,
    C: traits::Connection,
    Handler: handler::BranchHandler<RootMethod>,
> Processor<State, Role, RootMethod, C, Handler>
{
    pub async fn handle_loopback_request<'buf>(
        &mut self,
        write: &'buf mut Vec<u8>,
    ) -> Result<
        ResOf<'buf, RootMethod>,
        HandleLoopbackError<
            C,
            <replier::futures_io::Replier<RootMethod, C::SendStream> as traits::Replier<
                RootMethod,
            >>::Error,
        >,
    >
    where
        ReqOf<'buf, RootMethod>: minicbor::Decode<'buf, ()>,
    {
        let stream = self
            .connection
            .accept_stream()
            .await
            .map_err(HandleLoopbackError::Accept)?;

        let (send, recv) = stream;

        let replier = replier::futures_io::Replier::new(send);
        let receipt: replier::futures_io::Receipt<_> =
            crate::io::respond(write, recv, replier, &mut self.handler).await?;

        Ok(receipt.finalize().await?)
    }

    async fn handle_loopback_requests_inner<'buffer>(
        mut self,
    ) -> HandleLoopbackError<
        C,
        <replier::futures_io::Replier<RootMethod, C::SendStream> as traits::Replier<
            RootMethod,
        >>::Error,
    >
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
    ) -> super::ProcessorFut<
        impl Future<
            Output = HandleLoopbackError<
                C,
                <replier::futures_io::Replier<RootMethod, C::SendStream> as traits::Replier<
                    RootMethod,
                >>::Error,
            >,
        >,
    >
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
    {
        super::ProcessorFut(Box::pin(self.handle_loopback_requests_inner()))
    }
}

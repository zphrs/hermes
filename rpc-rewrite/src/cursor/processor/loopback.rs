//! Processor implementation for when the root method cannot transition to any other states.

use super::Processor;
use crate::io::Connection;

use crate::{
    io::read::read,
    traits::{
        self, Receipt as _, handler,
        method::{self, ReqOf, ResOf},
        replier,
    },
};
use std::convert::Infallible;

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
    ) -> Result<
        ResOf<'buf, RootMethod>,
        super::Error<
            C,
            <replier::futures_io::Replier<RootMethod, C::SendStream> as traits::Replier<
                RootMethod,
            >>::Error,
            Infallible,
        >,
    >
    where
        ReqOf<'buf, RootMethod>: minicbor::Decode<'buf, ()>,
    {
        let stream = self
            .connection
            .accept_stream()
            .await
            .map_err(super::Error::Accept)?;

        let (send, recv) = stream;

        let replier = replier::futures_io::Replier::new(send);
        let handler: &mut Handler = &mut self.handler;

        write.clear();
        let request: ReqOf<RootMethod> = read(write, recv).await?;
        let receipt = handler
            .handle::<_>(request, replier)
            .await
            .map_err(super::Error::Replier)?;

        match receipt.finalize().await {
            Ok(res) => Ok(res),
            Err(e) => match e {},
        }
    }

    async fn handle_loopback_requests_inner<'buffer>(mut self) -> HandleLoopbackError<C, RootMethod>
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
    ) -> super::ProcessorFut<impl Future<Output = HandleLoopbackError<C, RootMethod>>>
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
    {
        super::ProcessorFut::new(self.handle_loopback_requests_inner())
    }
}

pub type HandleLoopbackError<C, RootMethod> = super::Error<
    C,
    <replier::futures_io::Replier<RootMethod, <C as Connection>::SendStream> as traits::Replier<
        RootMethod,
    >>::Error,
    Infallible,
>;

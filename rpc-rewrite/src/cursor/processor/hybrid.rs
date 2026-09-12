mod possibly_delayed;

use std::{convert::Infallible, ops::Deref, pin::pin};

use futures::{FutureExt, StreamExt, select, stream::FuturesUnordered};
use tracing::debug;

use crate::{
    cursor::processor::{
        Processor, ProcessorFut,
        transition::{ProcessorTransition, ReplyPrimed, delayed_replier},
    },
    io::Connection,
    marker::{Branch, CanTransition},
    method::{self, ReqOf, ResOf, handler::can_transition, replier::Receipt},
};

type HandleHybridTransitionRequestResult<'a, State, Role, C, RootMethod, Handler> = Result<
    ProcessorTransition<
        State,
        Role,
        C,
        ReplyPrimed<
            ResOf<'a, RootMethod>,
            <C as Connection>::SendStream,
            <Handler as can_transition::BranchHandler<RootMethod>>::NextHandler,
        >,
    >,
    Error<C>,
>;
// TODO: implement debug
#[derive(thiserror::Error)]
pub enum Error<C: Connection> {
    #[error("could not accept stream")]
    Accept(#[source] C::AcceptError),
    #[error("could not read request")]
    Read(#[from] crate::io::read::Error<C::RecvStream>),
    #[error("could not encode transition response")]
    Encode(#[from] minicbor::encode::Error<Infallible>),
    #[error("could not write loopback response")]
    Write(#[from] crate::io::write::Error<C::SendStream>),
}

impl<C: Connection> std::fmt::Debug for Error<C>
where
    C::AcceptError: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Accept(arg0) => f.debug_tuple("Accept").field(arg0).finish(),
            Self::Read(arg0) => f.debug_tuple("Read").field(arg0).finish(),
            Self::Encode(arg0) => f.debug_tuple("Encode").field(arg0).finish(),
            Self::Write(arg0) => f.debug_tuple("Write").field(arg0).finish(),
        }
    }
}

impl<
    State,
    Role,
    RootMethod: method::OfType<Branch<CanTransition>>,
    C: Connection,
    Handler: can_transition::BranchHandler<RootMethod> + Clone,
> Processor<State, Role, RootMethod, C, Handler>
{
    pub fn handle_hybrid_concurrent_transition_requests<'buf>(
        self,
        buf: &'buf mut Vec<u8>,
    ) -> ProcessorFut<
        impl Future<
            Output = HandleHybridTransitionRequestResult<'buf, State, Role, C, RootMethod, Handler>,
        >,
    >
    where
        for<'a> <RootMethod as method::Method>::Req<'a>: minicbor::Decode<'a, ()>,
    {
        ProcessorFut::new(self.handle_hybrid_concurrent_transition_requests_inner(buf))
    }
    /// SAFETY: caller must ensure that the 0th field returned, the Vec<u8>
    /// is moved to a &'buf mut Vec<u8> reference and must ensure that
    /// the &'buf mut Vec<u8> reference is not overwritten with any other
    /// Vec<u8> value returned from this function for as long as the
    /// RootMethod::Res<'buf> reference lives for.
    async unsafe fn handle_stream<'buf>(
        stream: (C::SendStream, C::RecvStream),
        handler: Handler,
    ) -> Result<
        Option<(
            Vec<u8>,
            delayed_replier::Receipt<
                <RootMethod as crate::Method>::Res<'buf>,
                <C as Connection>::SendStream,
            >,
            Handler::NextHandler,
        )>,
        Error<C>,
    >
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
    {
        let replier: possibly_delayed::Replier<RootMethod, _> =
            possibly_delayed::Replier::new(stream.0);
        let out = {
            let mut buffer = Vec::new();
            let res = {
                let buf_borrow = &mut buffer;
                let (receipt, handler) = {
                    let request: ReqOf<RootMethod> = crate::io::read(buf_borrow, stream.1).await?;
                    handler.handle_possible_transition(request, replier).await?
                };
                match receipt {
                    possibly_delayed::Receipt::Immediate(res) => match res.finalize().await {
                        Err(e) => match e {},
                        Ok(_res) => None,
                    },
                    possibly_delayed::Receipt::Delayed(tres) => {
                        let tres: delayed_replier::Receipt<
                            <RootMethod as crate::Method>::Res<'_>,
                            <C as Connection>::SendStream,
                        > = tres;
                        // SAFETY: caller upholds guarantees that makes extending
                        // the lifetime of Res to 'buf safe
                        let transmuted = unsafe {
                            std::mem::transmute::<
                                delayed_replier::Receipt<
                                    ResOf<'_, RootMethod>,
                                    <C as Connection>::SendStream,
                                >,
                                delayed_replier::Receipt<
                                    ResOf<'buf, RootMethod>,
                                    <C as Connection>::SendStream,
                                >,
                            >(tres)
                        };
                        let res_1 = handler.unwrap();
                        Some((transmuted, res_1))
                    }
                }
            };
            res.map(|res| (buffer, res.0, res.1))
        };
        Result::<_, Error<C>>::Ok(out)
    }

    pub async fn handle_hybrid_concurrent_transition_requests_inner<'buf>(
        self,
        buf: &'buf mut Vec<u8>,
    ) -> HandleHybridTransitionRequestResult<'buf, State, Role, C, RootMethod, Handler>
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
    {
        let mut js = FuturesUnordered::new();
        let (receipt, next_handler) = {
            let mut stream_fut = pin!(self.connection.accept_stream().fuse());
            loop {
                select! {
                    stream = stream_fut => {
                        let stream = stream.map_err(Error::Accept)?;
                        let handler = self.handler.clone();
                        stream_fut.set(self.connection.accept_stream().fuse());
                        debug!("pushing to joinset");
                        js.push(unsafe {Self::handle_stream(stream, handler)});
                    }
                    res = js.select_next_some() => {
                        let res = res?;
                        match res {
                            Some((buffer, transition_res, next_handler)) => {
                                // SAFETY: js dropped here to ensure that we never
                                // execute this code again (would need to call
                                // js.next() to execute this block again) to ensure
                                // we uphold the safety of calling handle_stream
                                drop(js);
                                *buf = buffer;
                                break (transition_res, next_handler)
                            },
                            None => continue,
                        }
                    }
                }
            }
        };

        Ok(ProcessorTransition::new(
            self.connection,
            receipt,
            next_handler,
        ))
    }
}

trait GetBuffer {
    async fn get_buffer(&self) -> impl Deref<Target = [u8]>;
}

pub struct Allocator {
    buffer: [u8; 100000],
}

impl Allocator {
    pub fn new() -> Self {
        Self {
            buffer: [0u8; 100000],
        }
    }
}

impl GetBuffer for Allocator {
    async fn get_buffer(&self) -> impl Deref<Target = [u8]> {
        self.buffer.as_slice()
    }
}

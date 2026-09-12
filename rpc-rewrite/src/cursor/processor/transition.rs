use std::{convert::Infallible, fmt::Debug};

use super::Processor;
use crate::{
    cursor::{processor::ProcessorFut, requester::Requester, transition::CursorCredit},
    io::{Connection, write},
    marker::{Branch, NotApplicable, Transition},
    method::{self, ReqOf, ResOf, handler::TransitionBranchHandler},
};

pub mod delayed_replier;
use delayed_replier::DelayedReplier;

/// rpc processing follows these steps:
/// 1. accept stream
/// 2. [`Read`] request
/// 3. handle request (currently is infallible)
/// 4. reply response
#[derive(thiserror::Error)]
pub enum ConcurrentError<C: Connection, HandlerError> {
    #[error("could not accept stream")]
    Accept(#[source] C::AcceptError),
    #[error("could not read request")]
    Read(#[from] crate::io::read::Error<C::RecvStream>),
    #[error("handler error: {0}")]
    Handler(#[source] HandlerError),
    #[error("could not encode response")]
    Encode(#[from] minicbor::encode::Error<Infallible>),
}

impl<C: Connection, HandlerError: Debug> Debug for ConcurrentError<C, HandlerError>
where
    C::AcceptError: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Accept(arg0) => f.debug_tuple("Accept").field(arg0).finish(),
            Self::Read(arg0) => f.debug_tuple("Read").field(arg0).finish(),
            Self::Handler(arg0) => f.debug_tuple("Handler").field(arg0).finish(),
            Self::Encode(arg0) => f.debug_tuple("Encode").field(arg0).finish(),
        }
    }
}

/// differs from [`ConcurrentError`] only by replacing Write with Encode because
/// the handle_transition_request function calls finalize() which writes the
/// response to the wire.
#[derive(thiserror::Error)]
pub enum Error<C: Connection, HandlerError> {
    #[error("could not accept stream")]
    Accept(#[source] C::AcceptError),
    #[error("could not read request")]
    Read(#[from] crate::io::read::Error<C::RecvStream>),
    #[error("handler error: {0}")]
    Handler(#[source] HandlerError),
    #[error("could not write response")]
    Write(#[from] write::Error<C::SendStream>),
}

impl<C: Connection, HandlerError: Debug> Debug for Error<C, HandlerError>
where
    C::AcceptError: Debug,
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

impl<C: Connection, HandlerError: Debug> From<ConcurrentError<C, HandlerError>>
    for Error<C, HandlerError>
{
    fn from(value: ConcurrentError<C, HandlerError>) -> Self {
        match value {
            ConcurrentError::Accept(error) => Self::Accept(error),
            ConcurrentError::Read(error) => Self::Read(error),
            ConcurrentError::Handler(error) => Self::Handler(error),
            ConcurrentError::Encode(error) => Self::Write(write::Error::Encode(error)),
        }
    }
}

type HandleTransitionRequestResult<'a, State, Role, C, RootMethod, Handler> = Result<
    ProcessorTransition<
        State,
        Role,
        C,
        processor_transition::ReplyPrimed<
            ResOf<'a, RootMethod>,
            <C as Connection>::SendStream,
            <Handler as TransitionBranchHandler<RootMethod>>::NextHandler,
        >,
    >,
    ConcurrentError<C, Infallible>,
>;

pub use processor_transition::{Entrypoint, Finished, ProcessorTransition, ReplyPrimed};

impl<
    State,
    Role,
    RootMethod: method::OfType<Branch<Transition>>,
    C: Connection,
    Handler: TransitionBranchHandler<RootMethod>,
> Processor<State, Role, RootMethod, C, Handler>
{
    pub fn handle_concurrent_transition_request(
        self,
        write: &mut Vec<u8>,
    ) -> ProcessorFut<
        impl Future<Output = HandleTransitionRequestResult<'_, State, Role, C, RootMethod, Handler>>,
    >
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
    {
        ProcessorFut::new(self.handle_concurrent_transition_request_inner(write))
    }

    async fn handle_concurrent_transition_request_inner(
        self,
        write: &mut Vec<u8>,
    ) -> Result<
        ProcessorTransition<
            State,
            Role,
            C,
            processor_transition::ReplyPrimed<
                ResOf<'_, RootMethod>,
                C::SendStream,
                Handler::NextHandler,
            >,
        >,
        ConcurrentError<C, Infallible>,
    >
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
    {
        let stream = self
            .connection
            .accept_stream()
            .await
            .map_err(ConcurrentError::Accept)?;

        let delayed_replier: DelayedReplier<RootMethod, _> = DelayedReplier::new(stream.0);

        write.clear();
        let request: ReqOf<RootMethod> = crate::io::read::read(write, stream.1).await?;
        let (delayed_receipt, next_handler) = self
            .handler
            .handle_transition(request, delayed_replier)
            .await
            .map_err(ConcurrentError::Encode)?;

        Ok(ProcessorTransition::new(
            self.connection,
            delayed_receipt,
            next_handler,
        ))
    }

    pub async fn handle_transition_request(
        self,
        write: &mut Vec<u8>,
        requester: Requester<State, Role, NotApplicable, C>,
    ) -> Result<
        (
            ResOf<'_, RootMethod>,
            Handler::NextHandler,
            CursorCredit<State, Role, C>,
        ),
        Error<C, Infallible>,
    >
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
    {
        let processor_transition = self
            .handle_concurrent_transition_request_inner(write)
            .await?;
        // we're good to just transition; no tiebreak
        assert!(
            requester.conn().stable_id() == processor_transition.conn().stable_id(),
            "requester and processor MUST both belong to the same connection"
        );
        let ((res, next_handler), processor_transition) = processor_transition
            .reply()
            .await
            .map_err(write::Error::Send)?;
        // can avoid notifying because the other side shouldn't be expecting
        // a transition
        Ok((
            res,
            next_handler,
            CursorCredit::new(processor_transition.into_conn()),
        ))
    }
}

mod processor_transition;

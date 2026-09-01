pub mod buffers;
pub mod loopback;

use crate::{
    cursor::processor::transitions::processor_transition::{ProcessorTransition, ReplyPrimed},
    traits::{
        self,
        markers::NotApplicable,
        method::{self},
    },
};
pub use buffers::Buffers;
use futures::FutureExt;
use std::{
    convert::Infallible,
    future::{Pending, Ready, pending, ready},
    marker::PhantomData,
};

pub struct Processor<State, Role, RootMethod: method::Branch, C: traits::Connection, Handler> {
    connection: C,
    handler: Handler,
    _marker: PhantomData<(State, Role, RootMethod)>,
}

impl<State, Role, RootMethod: method::Branch, C: traits::Connection, Handler>
    Processor<State, Role, RootMethod, C, Handler>
{
    pub(crate) fn new(connection: C, handler: Handler) -> Self {
        Self {
            handler,
            connection,
            _marker: PhantomData,
        }
    }
}

/// marker struct for any future that takes ownership of a [`Processor`].
/// Dropping this future MUST drop the owned Processor as well.
#[pin_project::pin_project]
pub struct ProcessorFut<Fut>(#[pin] Fut);

impl<State, Role, RootMethod: traits::method::Branch, C: traits::Connection, Handler>
    From<Processor<State, Role, RootMethod, C, Handler>>
    for ProcessorFut<
        Pending<
            Result<
                ProcessorTransition<
                    State,
                    Role,
                    C,
                    ReplyPrimed<NotApplicable, C::SendStream, Handler>,
                >,
                Infallible,
            >,
        >,
    >
{
    fn from(_value: Processor<State, Role, RootMethod, C, Handler>) -> Self {
        ProcessorFut(pending())
    }
}

impl<Fut, Output> Future for ProcessorFut<Fut>
where
    Fut: Future<Output = Output>,
{
    type Output = Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut this = self.project();
        this.0.poll_unpin(cx)
    }
}

pub mod transitions {
    use std::{convert::Infallible, pin::pin};

    use super::Processor;
    use crate::{
        cursor::processor::ProcessorFut,
        traits::{
            self, Connection,
            handler::TransitionBranchHandler,
            io::BytesReadStream,
            method::{self, ReqOf, ResOf},
        },
    };

    #[derive(Debug, thiserror::Error)]
    pub enum HandleTransitionError<C: Connection> {
        #[error("accept: {0}")]
        Accept(C::AcceptError),
        #[error("respond: {0}")]
        Respond(
            #[from]
            crate::io::respond::Error<
                minicbor::encode::Error<Infallible>,
                <C::RecvStream as BytesReadStream>::Error,
            >,
        ),
    }

    pub mod delayed_replier;
    use delayed_replier::DelayedReplier;

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
        HandleTransitionError<C>,
    >;

    impl<
        State,
        Role,
        RootMethod: method::Branch + method::Transitions,
        C: traits::Connection,
        Handler: TransitionBranchHandler<RootMethod>,
    > Processor<State, Role, RootMethod, C, Handler>
    {
        pub fn handle_transition_request(
            self,
            write: &mut Vec<u8>,
        ) -> ProcessorFut<
            impl Future<Output = HandleTransitionRequestResult<'_, State, Role, C, RootMethod, Handler>>,
        >
        where
            for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
        {
            ProcessorFut(Box::pin(self.handle_transition_request_inner(write)))
        }

        async fn handle_transition_request_inner(
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
            HandleTransitionError<C>,
        >
        where
            for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
        {
            let stream = pin!(self.connection.accept_stream())
                .await
                .map_err(HandleTransitionError::Accept)?;

            let delayed_replier: DelayedReplier<RootMethod, _> = DelayedReplier::new(stream.0);

            let (delayed_receipt, next_handler) = pin!(crate::io::respond::transition(
                write,
                stream.1,
                delayed_replier,
                self.handler
            ))
            .await?;

            Ok(ProcessorTransition::new(
                self.connection,
                delayed_receipt,
                next_handler,
            ))
        }
    }

    pub mod processor_transition;

    use processor_transition::ProcessorTransition;
}

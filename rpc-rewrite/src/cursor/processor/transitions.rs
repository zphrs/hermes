use std::convert::Infallible;

use super::Processor;
use crate::{
    cursor::processor::ProcessorFut,
    io::Connection,
    method::{self, ReqOf, ResOf, handler::TransitionBranchHandler, replier::Replier},
};

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
    super::Error<C, minicbor::encode::Error<Infallible>, Infallible>,
>;

impl<
    State,
    Role,
    RootMethod: method::Branch + method::Transitions,
    C: Connection,
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
        ProcessorFut::new(self.handle_transition_request_inner(write))
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
        super::Error<
            C,
            <DelayedReplier<RootMethod, C::SendStream> as Replier<RootMethod>>::Error,
            Infallible,
        >,
    >
    where
        for<'a> ReqOf<'a, RootMethod>: minicbor::Decode<'a, ()>,
    {
        let stream = self
            .connection
            .accept_stream()
            .await
            .map_err(super::Error::Accept)?;

        let delayed_replier: DelayedReplier<RootMethod, _> = DelayedReplier::new(stream.0);

        write.clear();
        let request: ReqOf<RootMethod> = crate::io::read::read(write, stream.1).await?;
        let (delayed_receipt, next_handler) = self
            .handler
            .handle_transition(request, delayed_replier)
            .await
            .map_err(super::Error::Replier)?;

        Ok(ProcessorTransition::new(
            self.connection,
            delayed_receipt,
            next_handler,
        ))
    }
}

pub mod processor_transition;

use processor_transition::ProcessorTransition;

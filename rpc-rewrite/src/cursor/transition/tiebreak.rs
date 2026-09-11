use std::pin::Pin;

use futures::future::select;

use crate::{
    Method,
    cursor::{
        self,
        processor::{self, ProcessorFut},
        requester::transition::requester_transition,
        role,
        transition::{RequesterOrRequesterTransition, Won, next},
    },
    io,
    method::ResOf,
};

#[derive(Debug, thiserror::Error)]
pub enum Error<ProcessorError, RequesterError> {
    #[error("processor resolved to an error")]
    Processor(#[source] ProcessorError),
    #[error("requester resolved to an error")]
    Requester(#[source] RequesterError),
}

pub async fn tiebreak<
    'rbuf,
    'rreq,
    State,
    Role,
    C: io::Connection,
    ProcessorRes,
    NextHandler,
    RootRequest,
    RequesterMethod: Method,
    ProcessorError,
    RequesterError,
>(
    eventual_processor: &mut (
             impl Future<
        Output = Result<
            processor::transition::Entrypoint<State, Role, C, ProcessorRes, NextHandler>,
            ProcessorError,
        >,
    > + Unpin
         ),
    eventual_requester: &mut (
             impl Future<
        Output = Result<
            requester_transition::Entrypoint<'rbuf, State, Role, C, RootRequest, RequesterMethod>,
            RequesterError,
        >,
    > + Unpin
         ),
) -> Result<
    With<'rbuf, State, Role, C, ProcessorRes, NextHandler, RootRequest, RequesterMethod>,
    Error<ProcessorError, RequesterError>,
> {
    match select(eventual_processor, eventual_requester).await {
        futures::future::Either::Left((processor, _eventual_requester)) => {
            let processor = processor.map_err(Error::Processor)?;
            Ok(With::Processor(WithProcessor(processor)))
        }
        futures::future::Either::Right((requester, _eventual_processor)) => {
            let requester = requester.map_err(Error::Requester)?;
            Ok(With::Requester(WithRequester(requester)))
        }
    }
}

pub enum With<
    'rbuf,
    State,
    Role,
    C: io::Connection,
    ProcessorRes,
    NextHandler,
    RequesterRootRequest,
    RequesterMethod,
> {
    Processor(WithProcessor<State, Role, C, ProcessorRes, NextHandler>),
    Requester(WithRequester<'rbuf, State, Role, C, RequesterRootRequest, RequesterMethod>),
}

pub struct WithProcessor<State, Role, C: io::Connection, Res, NextHandler>(
    processor::transition::Entrypoint<State, Role, C, Res, NextHandler>,
);

pub struct WithRequester<'rbuf, State, Role, C: io::Connection, RootRequest, M>(
    requester_transition::Entrypoint<'rbuf, State, Role, C, RootRequest, M>,
);

#[expect(private_bounds, reason = "role")]
impl<'rbuf, State, Role: role::Sealed, C: io::Connection, RootRequest, M: Method>
    WithRequester<'rbuf, State, Role, C, RootRequest, M>
{
    pub async fn next<Fut, ProcessorRes, NextHandler, ProcessorError>(
        self,
        processor: Pin<&mut ProcessorFut<Fut>>,
    ) -> std::result::Result<
        (
            cursor::transition::Won<ProcessorRes, <M as Method>::Res<'rbuf>, NextHandler>,
            cursor::transition::SharedCredit<State, Role, C>,
        ),
        cursor::transition::next::NextError<C, ProcessorError>,
    >
    where
        for<'a> ResOf<'a, M>: minicbor::Decode<'a, ()>,
        Fut: Future<
            Output = std::result::Result<
                processor::transition::Entrypoint<State, Role, C, ProcessorRes, NextHandler>,
                ProcessorError,
            >,
        >,
    {
        let inner = self.0;
        next::with_requester_transition(inner, processor).await
    }
}

#[expect(private_bounds, reason = "role")]
impl<State, Role: role::Sealed, C: io::Connection, Res, NextHandler>
    WithProcessor<State, Role, C, Res, NextHandler>
{
    pub async fn next<
        'rbuf,
        'rreq,
        RequesterRootMethod: Method,
        RequesterMethod: Method,
        RequesterError,
    >(
        self,
        eventual_processor: impl Future<
            Output = std::result::Result<
                RequesterOrRequesterTransition<
                    'rbuf,
                    'rreq,
                    State,
                    Role,
                    RequesterRootMethod,
                    C,
                    RequesterMethod,
                >,
                RequesterError,
            >,
        >,
    ) -> Result<
        (
            Won<Res, <RequesterMethod as Method>::Res<'rbuf>, NextHandler>,
            cursor::transition::SharedCredit<State, Role, C>,
        ),
        cursor::transition::NextError<C, RequesterError>,
    >
    where
        <RequesterMethod as Method>::Res<'rbuf>: minicbor::Decode<'rbuf, ()>,
    {
        let inner = self.0;
        cursor::transition::next::with_processor_transition(inner, eventual_processor).await
    }
}

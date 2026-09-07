pub mod next;
mod requester_or_requester_transition;

pub use next::NextError;

use std::marker::PhantomData;

use futures::future::select;

use crate::traits;

use super::{
    processor::transitions::processor_transition::{self, ProcessorTransition},
    requester::transition::{RequesterTransition, requester_transition},
};

type RequesterTransitionEntrypoint<'buf, State, Role, C, RootRequest, M> = RequesterTransition<
    State,
    Role,
    C,
    requester_transition::Sent<'buf, RootRequest, <C as traits::Connection>::RecvStream, M>,
>;

pub use requester_or_requester_transition::RequesterOrRequesterTransition;
pub type ProcessorTransitionEntrypoint<State, Role, C, Res, NextHandler> = ProcessorTransition<
    State,
    Role,
    C,
    processor_transition::ReplyPrimed<Res, <C as traits::Connection>::SendStream, NextHandler>,
>;

pub async fn tiebreak<
    'rbuf,
    'rreq,
    State,
    Role,
    C: traits::Connection,
    Res,
    NextHandler,
    RootRequest,
    M,
    ProcessorError,
    RequesterError,
>(
    eventual_processor: &mut (
             impl Future<
        Output = Result<
            ProcessorTransitionEntrypoint<State, Role, C, Res, NextHandler>,
            ProcessorError,
        >,
    > + Unpin
         ),
    eventual_requester: &mut (
             impl Future<
        Output = Result<
            RequesterTransitionEntrypoint<'rbuf, State, Role, C, RootRequest, M>,
            RequesterError,
        >,
    > + Unpin
         ),
) -> TiebreakResult<
    'rbuf,
    State,
    Role,
    C,
    Res,
    NextHandler,
    RootRequest,
    M,
    ProcessorError,
    RequesterError,
> {
    match select(eventual_processor, eventual_requester).await {
        futures::future::Either::Left((processor_transition, _eventual_requester)) => {
            TiebreakResult::Processor(processor_transition)
        }
        futures::future::Either::Right((requester_transition, _eventual_processor)) => {
            TiebreakResult::Requester(requester_transition)
        }
    }
}

pub enum TiebreakResult<
    'rbuf,
    State,
    Role,
    C: traits::Connection,
    Res,
    NextHandler,
    RootRequest,
    M,
    ProcessorError,
    RequesterError,
> {
    Processor(
        Result<ProcessorTransitionEntrypoint<State, Role, C, Res, NextHandler>, ProcessorError>,
    ),
    Requester(
        Result<
            RequesterTransitionEntrypoint<'rbuf, State, Role, C, RootRequest, M>,
            RequesterError,
        >,
    ),
}

pub enum Won<PRes, RRes, NextHandler> {
    Processor {
        res: PRes,
        next_handler: NextHandler,
    },
    Requester {
        res: RRes,
    },
}

pub struct SharedCredit<State, Role, C> {
    _marker: PhantomData<(State, Role)>,
    connection: C,
}

impl<State, Role, C> SharedCredit<State, Role, C> {
    pub fn into_connection(self) -> C {
        self.connection
    }
}

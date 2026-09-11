pub mod next;
mod requester_or_requester_transition;
pub mod tiebreak;

pub use next::NextError;
pub use tiebreak::tiebreak;

use std::marker::PhantomData;

use crate::io;

use super::{
    processor::transitions::processor_transition::{self, ProcessorTransition},
    requester::transition::{RequesterTransition, requester_transition},
};

pub type RequesterTransitionEntrypoint<'buf, State, Role, C, RootRequest, M> = RequesterTransition<
    State,
    Role,
    C,
    requester_transition::Sent<'buf, RootRequest, <C as io::Connection>::RecvStream, M>,
>;

pub use requester_or_requester_transition::RequesterOrRequesterTransition;
pub type ProcessorTransitionEntrypoint<State, Role, C, Res, NextHandler> = ProcessorTransition<
    State,
    Role,
    C,
    processor_transition::ReplyPrimed<Res, <C as io::Connection>::SendStream, NextHandler>,
>;

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

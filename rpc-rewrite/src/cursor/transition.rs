pub mod next;
mod requester_or_requester_transition;
pub mod tiebreak;

pub use next::NextError;
pub use tiebreak::tiebreak;

use std::marker::PhantomData;

use super::requester::transition::requester_transition;

pub use requester_or_requester_transition::RequesterOrRequesterTransition;

pub enum Won<PRes, RRes, NextHandler> {
    Processor {
        res: PRes,
        next_handler: NextHandler,
    },
    Requester {
        res: RRes,
    },
}

pub struct CursorCredit<State, Role, C> {
    _marker: PhantomData<(State, Role)>,
    connection: C,
}

impl<State, Role, C> CursorCredit<State, Role, C> {
    pub(crate) fn new(connection: C) -> Self {
        Self {
            _marker: PhantomData,
            connection,
        }
    }
    pub(crate) fn into_connection(self) -> C {
        self.connection
    }
}

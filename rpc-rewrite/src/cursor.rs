use std::marker::PhantomData;

use crate::{
    cursor::transition::SharedCredit,
    traits::{
        self, BranchHandler,
        markers::{Client, NotApplicable, Server},
        role, state,
    },
};

#[expect(private_bounds, reason = "for role")]
pub struct Cursor<State: crate::traits::State, Role: role::Sealed, C: traits::io::Connection> {
    connection: C,
    _state: state::Wrapper<State>,
    _marker: PhantomData<Role>,
}

pub type ServerProcessor<State, C, H> =
    processor::Processor<State, Server, <State as state::State>::ServerHandles, C, H>;

pub type ServerRequester<State, C> =
    requester::Requester<State, Server, <State as state::State>::ClientHandles, C>;

pub type ServerPair<State, C, H> = (ServerProcessor<State, C, H>, ServerRequester<State, C>);

impl<State: crate::traits::State, C: traits::Connection + Clone> Cursor<State, Server, C> {
    pub fn into_processor_and_requester<H>(self, handler: H) -> ServerPair<State, C, H> {
        (
            processor::Processor::new(self.connection.clone(), handler),
            requester::Requester::new(self.connection),
        )
    }
}

pub type ClientProcessor<State, C, H> =
    processor::Processor<State, Client, <State as state::State>::ClientHandles, C, H>;

pub type ClientRequester<State, C> =
    requester::Requester<State, Client, <State as state::State>::ServerHandles, C>;

pub type ClientPair<State, C, H> = (ClientProcessor<State, C, H>, ClientRequester<State, C>);

impl<State: crate::traits::State, C: traits::Connection + Clone> Cursor<State, Client, C> {
    pub fn into_processor_and_requester<H>(self, handler: H) -> ClientPair<State, C, H> {
        (
            processor::Processor::new(self.connection.clone(), handler),
            requester::Requester::new(self.connection),
        )
    }
}

#[expect(private_bounds, reason = "for role")]
impl<State: crate::traits::state::Entrypoint, Role: role::Sealed, C: traits::io::Connection>
    Cursor<State, Role, C>
{
    pub fn new(connection: C) -> Self {
        Self {
            connection,
            _state: state::Wrapper::new(),
            _marker: PhantomData,
        }
    }
}

// no need for entrypoint bound on struct if the caller has a wrapper because
// a wrapper can only be created via a transition response
#[expect(private_bounds, reason = "for role")]
impl<State: state::State, Role: role::Sealed, C: traits::io::Connection> Cursor<State, Role, C> {
    pub fn from_cursor_credit<OldState>(
        cursor_credit: SharedCredit<OldState, Role, C>,
        new_state: state::Wrapper<State>,
    ) -> Self {
        Self {
            connection: cursor_credit.into_connection(),
            _state: new_state,
            _marker: PhantomData,
        }
    }
}

// no need for entrypoint bound on struct if the caller has a wrapper because
// a wrapper can only be created via a transition response
#[expect(private_bounds, reason = "for role")]
impl<
    State: state::State<ClientHandles = NotApplicable, ServerHandles = NotApplicable>,
    Role: role::Sealed,
    C: traits::io::Connection,
> Cursor<State, Role, C>
{
    pub async fn wait_to_close(self) -> Result<(), C::CloseError> {
        self.connection.wait_for_close().await
    }
}

pub mod processor;
pub mod requester;
pub mod transition;

#[cfg(test)]
mod tests;

//! Walks the state machine starting at a given entrypoint
//! [`State`](crate::traits::State)s. Correct usage of a walker requires
//! wrapping a [Connection](crate::transport::Connection) immediately after the
//! Connection is established. From then on, the methods defined on the
//! [MachineWalker] will ensure that both the client and the server stay in sync
//! in their walk across the various [State](crate::traits::State)s and dictate
//! all requests sent alongside how received messages are handled.

// mod concurrent_request_handler;
pub mod processor;
mod requester;

#[cfg(test)]
mod test;
pub mod transition;

pub use transition::PendingTransitionReceipt;

pub use processor::{EventualTransitionRequest, Processor};
pub use requester::Requester;

pub use processor::TransitionRequestError;

use crate::{
    Method,
    state::Wrapper,
    traits::{self, state},
    transport::CallerExt,
};
#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct MachineCursor<
    State: crate::traits::State,
    Connection: crate::transport::Connection,
    Role: state::Role,
> {
    state_wrapper: state::Wrapper<State>,
    conn: Connection,
    _role: Role,
}

pub type MachineCursorClient<State, Connection> =
    MachineCursor<State, Connection, state::role::Client>;

pub type MachineCursorServer<State, Connection> =
    MachineCursor<State, Connection, state::role::Server>;

#[derive(Debug, thiserror::Error)]
pub enum FromSplitTransitionReceiptError<ClientError, ReplierError> {
    #[error("client: {0}")]
    Client(ClientError),
    #[error("replier: {0}")]
    Replier(ReplierError),
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<State: crate::traits::State, Connection: crate::transport::Connection, Role: state::Role>
    MachineCursor<State, Connection, Role>
{
    fn new_with_role(conn: Connection, role: Role, wrapper: state::Wrapper<State>) -> Self {
        Self {
            state_wrapper: wrapper,
            conn,
            _role: role,
        }
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct SplitReceipt<Connection: crate::transport::Connection, Role: state::role::Role>(
    Connection,
    Role,
);

impl<State: crate::traits::State, Connection: crate::transport::Connection + Clone + CallerExt>
    MachineCursor<State, Connection, state::role::Client>
where
    State::ClientHandles: Method,
    State::ServerHandles: Method,
{
    pub fn new(conn: Connection) -> Self {
        Self {
            state_wrapper: Wrapper::new_without_check(),
            conn,
            _role: state::role::Client,
        }
    }
    pub fn into_children_with_handler<
        'h,
        Handler: traits::Handler<State::ClientHandles, State::ClientHandles>,
    >(
        self,
        handler: &'h mut Handler,
    ) -> (
        Processor<'h, State, state::role::Client, State::ClientHandles, Connection, Handler>,
        Requester<State, state::role::Client, State::ServerHandles, Connection>,
    )
    where
        State: 'h,
    {
        let handler = Processor::new(
            self.state_wrapper.duplicate(),
            state::role::Client,
            handler,
            self.conn.clone(),
        );

        let sender = Requester::new(&self.state_wrapper, state::role::Client, self.conn.clone());

        (handler, sender)
    }
}

impl<State: crate::traits::State, Connection: crate::transport::Connection + Clone + CallerExt>
    MachineCursor<State, Connection, state::role::Server>
where
    State::ClientHandles: Method,
    State::ServerHandles: Method,
{
    pub fn new(conn: Connection) -> Self {
        Self {
            state_wrapper: state::Wrapper::new_without_check(),
            conn,
            _role: state::role::Server,
        }
    }
    pub fn into_children_with_handler<
        'h,
        Handler: traits::Handler<State::ServerHandles, State::ServerHandles>,
    >(
        self,
        handler: &'h mut Handler,
    ) -> (
        Processor<'h, State, state::role::Server, State::ServerHandles, Connection, Handler>,
        Requester<State, state::role::Server, State::ClientHandles, Connection>,
    )
    where
        State: 'h,
    {
        let handler = Processor::new(
            self.state_wrapper.duplicate(),
            state::role::Server,
            handler,
            self.conn.clone(),
        );

        let sender = Requester::new(&self.state_wrapper, state::role::Server, self.conn.clone());

        (handler, sender)
    }
}

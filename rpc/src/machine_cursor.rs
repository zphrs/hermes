//! Walks the state machine starting at a given entrypoint
//! [`State`](crate::traits::State)s. Correct usage of a walker requires
//! wrapping a [Connection](crate::transport::Connection) immediately after the
//! Connection is established. From then on, the methods defined on the
//! [MachineWalker] will ensure that both the client and the server stay in sync
//! in their walk across the various [State](crate::traits::State)s and dictate
//! all requests sent alongside how received messages are handled.

// mod concurrent_request_handler;
mod sender;
mod state_handler;
#[cfg(test)]
mod test;

use sender::Sender;

use crate::{
    Method,
    machine_cursor::{
        sender::TransitionReceipt,
        state_handler::{FinalizeFuture, PendingTransitionReceipt, StateHandler},
    },
    traits::{self, state},
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

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<State: crate::traits::State, Connection: crate::transport::Connection, Role: state::Role>
    MachineCursor<State, Connection, Role>
{
    pub fn new(conn: Connection, role: Role) -> Self {
        Self {
            state_wrapper: Default::default(),
            conn,
            _role: role,
        }
    }

    pub async fn from_split_receipt(
        split_receipt: SplitReceipt<Connection, Role>,
        new_state: state::Wrapper<State>,
    ) -> Result<Self, std::io::Error> {
        let _ = new_state;
        let SplitReceipt(finalize_promise, conn, role) = split_receipt;
        let out = Self::new(conn, role);
        finalize_promise.await?;
        Ok(out)
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct SplitReceipt<Connection: crate::transport::Connection, Role: state::role::Role>(
    FinalizeFuture<Connection::SendStream>,
    Connection,
    Role,
);

impl<State: crate::traits::State, Connection: crate::transport::Connection + Clone>
    MachineCursor<State, Connection, state::role::Client>
where
    State::ServerMethod: Method,
{
    pub fn into_sender(self) -> Sender<state::role::Client, State::ServerMethod, Connection> {
        let sender = Sender::new(&self.state_wrapper, state::role::Client, self.conn.clone());
        sender
    }
}

impl<State: crate::traits::State, Connection: crate::transport::Connection + Clone>
    MachineCursor<State, Connection, state::role::Client>
where
    State::ClientMethod: Method,
    State::ServerMethod: Method,
{
    pub fn into_parts<Handler: traits::Handler<State::ClientMethod>>(
        self,
        handler: Handler,
    ) -> (
        StateHandler<state::role::Client, State::ClientMethod, Connection, Handler>,
        Sender<state::role::Client, State::ServerMethod, Connection>,
    ) {
        let handler = StateHandler::new(
            &self.state_wrapper,
            state::role::Client,
            handler,
            self.conn.clone(),
        );

        let sender = Sender::new(&self.state_wrapper, state::role::Client, self.conn.clone());

        (handler, sender)
    }

    /// requires passing in the [`Sender`] linked to the [`StateHandler`] that
    /// returned the [`DelayedReceipt`] to ensure that a transition is only
    /// approved after all pending requests for the previous state was
    /// completed.
    #[must_use]
    pub fn split_transition_receipt(
        delayed_transition_receipt: PendingTransitionReceipt<
            Connection::SendStream,
            State::ClientMethod,
            state::role::Client,
            Connection,
        >,
        sender: Sender<state::role::Client, State::ServerMethod, Connection>,
    ) -> (
        <State::ClientMethod as Method>::Res,
        SplitReceipt<Connection, state::role::Client>,
    )
    where
        Connection::SendStream: futures::AsyncWrite + Unpin + 'static,
        State::ClientMethod: Method + 'static,
        State::ServerMethod: Method,
        Connection: PartialEq + std::fmt::Debug,
    {
        let (delayed_receipt, _role, handler_conn) = delayed_transition_receipt.into_parts();
        let (role, sender_conn) = sender.into_parts();
        assert_eq!(&*handler_conn, &sender_conn);
        let (res, actually_send) = delayed_receipt.finalize();
        (res, SplitReceipt(actually_send, sender_conn, role))
    }

    pub fn from_transition_receipt<
        NewState: crate::traits::State,
        H: traits::Handler<State::ClientMethod>,
        T,
    >(
        receipt: TransitionReceipt<T, state::role::Client, Connection>,
        wrapper: state::Wrapper<NewState>,
        handler: StateHandler<state::role::Client, State::ClientMethod, Connection, H>,
    ) -> MachineCursor<NewState, Connection, state::role::Client> {
        // we take in handler to ensure that we stop handling requests
        // from the old state.
        let _ = handler;
        MachineCursor {
            state_wrapper: wrapper,
            conn: receipt.into_connection(),
            _role: state::role::Client,
        }
    }
}

impl<State: crate::traits::State, Connection: crate::transport::Connection + Clone>
    MachineCursor<State, Connection, state::role::Server>
where
    State::ClientMethod: Method,
    State::ServerMethod: Method,
{
    pub fn into_parts<Handler: traits::Handler<State::ServerMethod>>(
        self,
        handler: Handler,
    ) -> (
        StateHandler<state::role::Server, State::ServerMethod, Connection, Handler>,
        Sender<state::role::Server, State::ClientMethod, Connection>,
    ) {
        let handler = StateHandler::new(
            &self.state_wrapper,
            state::role::Server,
            handler,
            self.conn.clone(),
        );

        let sender = Sender::new(&self.state_wrapper, state::role::Server, self.conn.clone());

        (handler, sender)
    }

    /// requires passing in the [`Sender`] linked to the [`StateHandler`] that
    /// returned the [`DelayedReceipt`] to ensure that a transition is only
    /// approved after all pending requests for the previous state was
    /// completed.
    #[must_use]
    pub fn split_transition_receipt(
        delayed_transition_receipt: PendingTransitionReceipt<
            Connection::SendStream,
            State::ServerMethod,
            state::role::Server,
            Connection,
        >,
        sender: Sender<state::role::Server, State::ClientMethod, Connection>,
    ) -> (
        <State::ServerMethod as Method>::Res,
        SplitReceipt<Connection, state::role::Server>,
    )
    where
        Connection::SendStream: futures::AsyncWrite + Unpin + 'static,
        State::ClientMethod: Method,
        State::ServerMethod: Method + 'static,
        Connection: PartialEq + std::fmt::Debug,
    {
        let (delayed_receipt, _role, handler_conn) = delayed_transition_receipt.into_parts();
        let (role, sender_conn) = sender.into_parts();
        assert_eq!(&*handler_conn, &sender_conn);
        let (res, actually_send) = delayed_receipt.finalize();
        (res, SplitReceipt(actually_send, sender_conn, role))
    }

    pub fn from_transition_receipt<
        NewState: crate::traits::State,
        H: traits::Handler<State::ServerMethod>,
        T,
    >(
        receipt: TransitionReceipt<T, state::role::Server, Connection>,
        wrapper: state::Wrapper<NewState>,
        handler: StateHandler<state::role::Server, State::ServerMethod, Connection, H>,
    ) -> MachineCursor<NewState, Connection, state::role::Server> {
        // we take in handler to ensure that we stop handling requests
        // from the old state.
        let _ = handler;
        MachineCursor {
            state_wrapper: wrapper,
            conn: receipt.into_connection(),
            _role: state::role::Server,
        }
    }
}

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
    CallerError, Method,
    machine_cursor::{
        sender::TransitionReceipt,
        state_handler::{PendingTransitionReceipt, StateHandler},
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

pub(super) mod requester_transitioned {
    use maxlen::MaxLen;

    use crate::traits::method::{can_transition, not_applicable::NotApplicable};

    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct Notification;

    pub struct Method;

    impl crate::Method for Method {
        type Req = Notification;

        type Res = NotApplicable;

        type CanTransition = can_transition::False;
    }
}

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
    pub fn new(conn: Connection, role: Role) -> Self {
        Self {
            state_wrapper: Default::default(),
            conn,
            _role: role,
        }
    }
    /// Waits for an acknowledgement from the sender that the view change
    /// went through on the other end before returning a new MachineCursor.
    pub async fn from_split_receipt(
        split_receipt: SplitReceipt<Connection, Role>,
        new_state: state::Wrapper<State>,
    ) -> Result<
        Self,
        FromSplitTransitionReceiptError<
            <Connection as crate::transport::Client>::Error,
            minicbor_io::Error,
        >,
    > {
        let _ = new_state;
        let SplitReceipt(conn, role) = split_receipt;
        let mut stream = conn
            .accept_stream()
            .await
            .map_err(FromSplitTransitionReceiptError::Client)?;
        // See [`MachineCursor::from_transition_receipt`] for where the request
        // is sent.
        let _notif = conn
            .handle_one_notification::<requester_transitioned::Method>(&mut stream)
            .await
            .map_err(FromSplitTransitionReceiptError::Replier)?;
        let out = Self::new(conn, role);
        Ok(out)
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
    /// completed. Sends off the transition confirmation. Use
    /// [`MachineCursor::from_split_receipt`] to construct a new cursor using
    /// the returned [`SplitReceipt`].
    #[must_use]
    pub async fn split_transition_receipt<'a>(
        delayed_transition_receipt: PendingTransitionReceipt<
            'a,
            Connection::SendStream,
            State::ClientMethod,
            state::role::Client,
            Connection,
        >,
        sender: Sender<state::role::Client, State::ServerMethod, Connection>,
    ) -> Result<
        (
            <State::ClientMethod as Method>::Res,
            SplitReceipt<Connection, state::role::Client>,
        ),
        std::io::Error,
    >
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
        actually_send.await?;
        Ok((res, SplitReceipt(sender_conn, role)))
    }

    pub async fn from_transition_receipt<
        NewState: crate::traits::State,
        H: traits::Handler<State::ClientMethod>,
        T,
    >(
        receipt: TransitionReceipt<T, state::role::Client, Connection>,
        wrapper: state::Wrapper<NewState>,
        handler: StateHandler<state::role::Client, State::ClientMethod, Connection, H>,
    ) -> Result<
        MachineCursor<NewState, Connection, state::role::Client>,
        CallerError<<Connection as crate::Caller>::Error>,
    > {
        // we take in handler to ensure that we stop handling requests
        // from the old state.
        let _ = handler;

        let conn = receipt.into_connection();
        // notify the server that we've officially transitioned and we're ready
        // to field requests.
        conn.notify::<requester_transitioned::Method, requester_transitioned::Notification>(
            requester_transitioned::Notification,
        )
        .await?;

        Ok(MachineCursor {
            state_wrapper: wrapper,
            conn,
            _role: state::role::Client,
        })
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
    pub async fn split_transition_receipt<'a>(
        delayed_transition_receipt: PendingTransitionReceipt<
            'a,
            Connection::SendStream,
            State::ServerMethod,
            state::role::Server,
            Connection,
        >,
        sender: Sender<state::role::Server, State::ClientMethod, Connection>,
    ) -> Result<
        (
            <State::ServerMethod as Method>::Res,
            SplitReceipt<Connection, state::role::Server>,
        ),
        std::io::Error,
    >
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
        actually_send.await?;
        Ok((res, SplitReceipt(sender_conn, role)))
    }

    pub async fn from_transition_receipt<
        NewState: crate::traits::State,
        H: traits::Handler<State::ServerMethod>,
        T,
    >(
        receipt: TransitionReceipt<T, state::role::Server, Connection>,
        wrapper: state::Wrapper<NewState>,
        handler: StateHandler<state::role::Server, State::ServerMethod, Connection, H>,
    ) -> Result<
        MachineCursor<NewState, Connection, state::role::Server>,
        CallerError<<Connection as crate::Caller>::Error>,
    > {
        // we take in handler to ensure that we stop handling requests
        // from the old state.
        let _ = handler;

        let conn = receipt.into_connection();

        conn.notify::<requester_transitioned::Method, requester_transitioned::Notification>(
            requester_transitioned::Notification,
        )
        .await?;

        Ok(MachineCursor {
            state_wrapper: wrapper,
            conn,
            _role: state::role::Server,
        })
    }
}

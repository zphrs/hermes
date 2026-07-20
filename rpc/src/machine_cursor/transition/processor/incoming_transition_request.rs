use crate::{
    Method,
    machine_cursor::{self, SplitReceipt, transition::processor::delayed_replier::DelayedReceipt},
    traits,
};

use super::delayed_replier::FinalizeFuture;

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct PendingTransitionReceipt<
    State: traits::Prioritized,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Client: crate::transport::Client,
>(
    DelayedReceipt<OldMethod>,
    Role,
    Client,
    traits::state::Wrapper<State>,
    State::Priority,
    Client::SendStream,
);

impl<
    State: traits::Prioritized,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Client: crate::transport::Client,
> PendingTransitionReceipt<State, OldMethod, Role, Client>
{
    pub fn new(
        delayed_receipt: DelayedReceipt<OldMethod>,
        role: Role,
        client: Client,
        wrapper: traits::state::Wrapper<State>,
        priority: State::Priority,
        sender: Client::SendStream,
    ) -> Self {
        Self(delayed_receipt, role, client, wrapper, priority, sender)
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<
    State: traits::Prioritized,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Connection: crate::transport::Connection + PartialEq + std::fmt::Debug,
> PendingTransitionReceipt<State, OldMethod, Role, Connection>
{
    /// requires passing in the [`Sender`] linked to the [`StateHandler`] that
    /// returned the [`DelayedReceipt`] to ensure that a transition is only
    /// approved after all pending requests for the previous state was
    /// completed. Sends off the transition confirmation. Use
    /// [`MachineCursor::from_split_receipt`] to construct a new cursor using
    /// the returned [`SplitReceipt`].
    pub fn split(
        self,
        sender: machine_cursor::requester::Requester<State, Role, State::ClientMethod, Connection>,
        should_tiebreak: bool,
    ) -> (
        OldMethod::Res,
        State::Priority,
        FinalizeFuture<Connection::SendStream>,
        SplitReceipt<Connection, Role>,
    )
    where
        Connection::SendStream: futures::AsyncWrite + Unpin,
        State::ClientMethod: Method,
        State::ServerMethod: Method,
    {
        let (delayed_receipt, _role, handler_conn, _wrapper, priority, send_stream) =
            self.into_parts();
        let (role, sender_conn) = sender.into_parts();
        assert_eq!(&handler_conn, &sender_conn);
        let (res, actually_send) = delayed_receipt.finalize(send_stream, should_tiebreak);

        (
            res,
            priority,
            actually_send,
            SplitReceipt(sender_conn, role),
        )
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<
    State: traits::Prioritized,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Client: crate::transport::Client,
> PendingTransitionReceipt<State, OldMethod, Role, Client>
{
    pub(crate) fn into_parts(
        self,
    ) -> (
        DelayedReceipt<OldMethod>,
        Role,
        Client,
        crate::state::Wrapper<State>,
        State::Priority,
        Client::SendStream,
    ) {
        (self.0, self.1, self.2, self.3, self.4, self.5)
    }
}

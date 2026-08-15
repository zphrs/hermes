use super::delayed_replier::DelayedReceipt;
use crate::traits;

pub(crate) struct PendingTransitionReceipt<
    State: traits::Prioritized,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Client: crate::transport::Client,
>(
    DelayedReceipt<OldMethod>,
    Role,
    Client,
    traits::state::Wrapper<State>,
    Option<State::Priority>,
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
        Self(
            delayed_receipt,
            role,
            client,
            wrapper,
            Some(priority),
            sender,
        )
    }

    pub(crate) fn conn(&self) -> &Client {
        &self.2
    }

    pub(crate) fn take_priority(
        &mut self,
    ) -> Option<<State as crate::state::Prioritized>::Priority> {
        self.4.take()
    }
}

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
        Option<State::Priority>,
        Client::SendStream,
    ) {
        (self.0, self.1, self.2, self.3, self.4, self.5)
    }
}

//! 1. Handle requests until a transition request arrives (resolves to IncomingTransitionRequest)
//! 2. either provide a Requester or a RequestTransition future.
//! 3. if provided a Requester:
//!     1. reply with the result and with the tiebreak flag unset and discard the requester.
//! 2. else if provided with a RequestTransition:
//!     1. tiebreak
//!     2. if tiebroken in the direction of the RequestTransition:
//!         1. break out of the processor tiebreaking (discard the IncomingTransitionRequest) and wait for the RequestTransition to finish
//!     4. else if tiebroken in the direction of the IncomingTransitionRequest:
//!         1. send off the IncomingTransitionRequest with the tiebreak flag set
//! 4. wait for the notification that the remote requester has fully transitioned

pub(self) mod delayed_replier;
mod incoming_transition_request;
pub(super) use delayed_replier::FinalizeFuture;
pub use incoming_transition_request::PendingTransitionReceipt;

pub use delayed_replier::{DelayedReceipt, DelayedReplier};

use crate::{
    MachineCursor,
    machine_cursor::{
        Requester,
        transition::requester::{AssertSacrificeError, ToSacrifice, assert_remote_sacrifice},
    },
    state::{self, Prioritized},
    traits, transport,
};

pub struct ProcessorTransition<Stage> {
    state: Stage,
}

impl<Stage> ProcessorTransition<Stage> {
    pub(crate) fn into_inner(self) -> Stage {
        self.state
    }
}

#[expect(private_bounds, reason = "for role")]
pub struct Entrypoint<
    State: traits::Prioritized,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Client: crate::transport::Client,
>(PendingTransitionReceipt<State, OldMethod, Role, Client>);
#[expect(private_bounds, reason = "for role")]
impl<
    'a,
    State: traits::Prioritized,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Client: crate::transport::Client,
> Entrypoint<State, OldMethod, Role, Client>
{
    pub(crate) fn into_parts(
        self,
    ) -> (
        DelayedReceipt<OldMethod>,
        Role,
        Client,
        state::Wrapper<State>,
        Option<<State as Prioritized>::Priority>,
        Client::SendStream,
    ) {
        self.0.into_parts()
    }

    pub(crate) fn into_inner(self) -> PendingTransitionReceipt<State, OldMethod, Role, Client> {
        self.0
    }

    pub(crate) fn inner_mut(
        &mut self,
    ) -> &mut PendingTransitionReceipt<State, OldMethod, Role, Client> {
        &mut self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NextWithRequesterError<Client> {
    Io(#[from] std::io::Error),
    Minicbor(#[from] minicbor_io::Error),
    Client(Client),
}

impl<Caller> From<AssertSacrificeError<Caller>> for NextWithRequesterError<Caller> {
    fn from(value: AssertSacrificeError<Caller>) -> Self {
        match value {
            AssertSacrificeError::AcceptStream(c) => Self::Client(c),
            AssertSacrificeError::Handler(error) => Self::Minicbor(error),
        }
    }
}

pub struct NeedWrapper<Conn, Role, Res> {
    conn: Conn,
    role: Role,
    res: Res,
}
impl<Conn, Role> NeedWrapper<Conn, Role, ()> {
    fn into_parts(self) -> (Conn, Role) {
        (self.conn, self.role)
    }
}

#[expect(private_bounds, reason = "for role")]
impl<
    'a,
    State: traits::Prioritized,
    ProcessorMethod: traits::Method,
    Role: traits::state::Role,
    Conn: crate::transport::Client,
> ProcessorTransition<Entrypoint<State, ProcessorMethod, Role, Conn>>
{
    pub fn new(
        incoming_transition_receipt: PendingTransitionReceipt<State, ProcessorMethod, Role, Conn>,
    ) -> Self {
        ProcessorTransition {
            state: Entrypoint(incoming_transition_receipt),
        }
    }
}

#[expect(private_bounds, reason = "for role")]
impl<
    'a,
    State: traits::Prioritized,
    ProcessorMethod: traits::Method,
    Role: traits::state::Role,
    Conn: crate::transport::Connection,
> ProcessorTransition<Entrypoint<State, ProcessorMethod, Role, Conn>>
{
    pub async fn next_with_requester<RequesterMethod: crate::Method>(
        self,
        requester: Requester<State, Role, RequesterMethod, Conn>,
        to_sacrifice: ToSacrifice,
    ) -> Result<
        ProcessorTransition<NeedWrapper<Conn, Role, ProcessorMethod::Res>>,
        NextWithRequesterError<<Conn as crate::transport::Client>::Error>,
    > {
        drop(to_sacrifice);
        let (receipt, role, client, _wrapper, priority, sender) = self.state.into_parts();
        // don't need priority because we have the whole requester so we know
        // there can't possibly be a conflict
        drop(priority);
        let (res, finalize_fut) = receipt.finalize(sender, false);
        let mut conn = requester.into_parts().1;
        assert!(
            client == conn,
            "requester and processor must belong to the same connection"
        );
        finalize_fut.await?;
        assert_remote_sacrifice(&mut conn).await?;

        Ok(ProcessorTransition {
            state: NeedWrapper { conn, role, res },
        })
    }
}

impl<Conn, Role, Res> ProcessorTransition<NeedWrapper<Conn, Role, Res>> {
    pub fn extract_res(self) -> (Res, ProcessorTransition<NeedWrapper<Conn, Role, ()>>) {
        (
            self.state.res,
            ProcessorTransition {
                state: NeedWrapper {
                    res: (),
                    conn: self.state.conn,
                    role: self.state.role,
                },
            },
        )
    }
}

#[expect(private_bounds, reason = "for role")]
impl<Conn: transport::Connection, Role: state::Role>
    ProcessorTransition<NeedWrapper<Conn, Role, ()>>
{
    pub fn finish<NewState: crate::State>(
        self,
        wrapper: state::Wrapper<NewState>,
    ) -> MachineCursor<NewState, Conn, Role> {
        let (conn, role) = self.state.into_parts();
        MachineCursor::new_with_role(conn, role, wrapper)
    }
}

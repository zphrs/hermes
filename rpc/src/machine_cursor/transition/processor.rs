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

mod delayed_replier;
mod incoming_transition_request;
pub(super) use delayed_replier::FinalizeFuture;
pub(crate) use incoming_transition_request::PendingTransitionReceipt;

pub use delayed_replier::{DelayedReceipt, DelayedReplier};
use tracing::trace;

use crate::{
    MachineCursor,
    machine_cursor::{
        Requester,
        transition::requester::{AssertSacrificeError, assert_remote_sacrifice},
    },
    state::{self, Prioritized, role},
    traits, transport,
};

pub struct ProcessorTransition<Stage> {
    state: Stage,
}

pub type ProcessorTransitionServerEntrypoint<State, Connection> = ProcessorTransition<
    StageOne<State, <State as crate::State>::ServerHandles, role::Server, Connection>,
>;

impl<Stage> ProcessorTransition<Stage> {
    pub(crate) fn into_inner(self) -> Stage {
        self.state
    }

    pub(crate) fn inner_mut(&mut self) -> &mut Stage {
        &mut self.state
    }
}

#[expect(private_bounds, reason = "for role")]
pub struct StageOne<
    State: traits::Prioritized,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Client: crate::transport::Client,
>(PendingTransitionReceipt<State, OldMethod, Role, Client>);
#[expect(private_bounds, reason = "for role")]
impl<
    State: traits::Prioritized,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Client: crate::transport::Client,
> StageOne<State, OldMethod, Role, Client>
{
    #[allow(clippy::type_complexity)]
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
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("minicbor: {0}")]
    Minicbor(#[from] minicbor_io::Error),
    #[error("client: {0}")]
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

pub struct StageTwo<Conn, Role, Res> {
    conn: Conn,
    role: Role,
    res: Res,
}
impl<Conn, Role> StageTwo<Conn, Role, ()> {
    fn into_parts(self) -> (Conn, Role) {
        (self.conn, self.role)
    }
}

#[expect(private_bounds, reason = "for role")]
impl<
    State: traits::Prioritized,
    ProcessorMethod: traits::Method,
    Role: traits::state::Role,
    Conn: crate::transport::Client,
> ProcessorTransition<StageOne<State, ProcessorMethod, Role, Conn>>
{
    pub(crate) fn new(
        incoming_transition_receipt: PendingTransitionReceipt<State, ProcessorMethod, Role, Conn>,
    ) -> Self {
        ProcessorTransition {
            state: StageOne(incoming_transition_receipt),
        }
    }
}

#[expect(private_bounds, reason = "for role")]
impl<
    State: traits::Prioritized,
    ProcessorMethod: traits::Method,
    Role: traits::state::Role,
    Conn: crate::transport::Connection,
> ProcessorTransition<StageOne<State, ProcessorMethod, Role, Conn>>
{
    pub async fn next_with_requester<RequesterMethod: crate::Method>(
        self,
        requester: Requester<State, Role, RequesterMethod, Conn>,
    ) -> Result<
        ProcessorTransition<StageTwo<Conn, Role, ProcessorMethod::Res>>,
        NextWithRequesterError<<Conn as crate::transport::Client>::Error>,
    > {
        let (receipt, role, client, _wrapper, priority, sender) = self.state.into_parts();
        // don't need priority because we have the whole requester so we know
        // there can't possibly be a conflict
        drop(priority);
        trace!("finalizing receipt");
        let (res, finalize_fut) = receipt.finalize(sender, false);
        let mut conn = requester.into_parts().1;
        assert!(
            client == conn,
            "requester and processor must belong to the same connection"
        );
        finalize_fut.await?;
        trace!("finalized receipt");
        assert_remote_sacrifice::<State, Role, _>(&mut conn).await?;

        Ok(ProcessorTransition {
            state: StageTwo { conn, role, res },
        })
    }
}

impl<Conn, Role, Res> ProcessorTransition<StageTwo<Conn, Role, Res>> {
    #[must_use]
    pub fn extract_res(self) -> (Res, ProcessorTransition<StageTwo<Conn, Role, ()>>) {
        (
            self.state.res,
            ProcessorTransition {
                state: StageTwo {
                    res: (),
                    conn: self.state.conn,
                    role: self.state.role,
                },
            },
        )
    }
}

#[expect(private_bounds, reason = "for role")]
impl<Conn: transport::Connection, Role: state::Role> ProcessorTransition<StageTwo<Conn, Role, ()>> {
    pub fn finish<NewState: crate::State>(
        self,
        wrapper: state::Wrapper<NewState>,
    ) -> MachineCursor<NewState, Conn, Role> {
        let (conn, role) = self.state.into_parts();
        MachineCursor::new_with_role(conn, role, wrapper)
    }
}

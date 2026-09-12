use crate::{
    cursor::{
        processor::{self, transition::delayed_replier::TransitionReply},
        requester::transition::{RequesterTransition, requester_transition::Finished},
        role,
        transition::{CursorCredit, Won, next::ProcessorSacrificed},
    },
    io::{Connection, notify, read, write},
};

use std::marker::PhantomData;
use tracing::trace;

#[derive(thiserror::Error)]
pub enum Error<C: Connection> {
    #[error("while reading request to transition: {0}")]
    Read(#[from] read::Error<C::RecvStream>),
    #[error("could not write transition response")]
    Write(#[from] write::Error<C::SendStream>),
    #[error("could not notify of sacrifice")]
    SendNotification(#[from] crate::io::notify::SendError<C>),
    #[error("while waiting for notification of transition")]
    ReceiveNotification(#[from] crate::io::notify::RecvError<C>),
}
pub(super) async fn definite_tiebreak_fn<
    State,
    Role: role::Sealed,
    C: Connection,
    PRes,
    RRes,
    NextHandler,
>(
    mut processor_transition: processor::transition::Entrypoint<State, Role, C, PRes, NextHandler>,
    recv_fut: impl Future<
        Output = Result<
            (
                TransitionReply<RRes>,
                RequesterTransition<State, Role, C, Finished>,
            ),
            crate::io::read::Error<<C as Connection>::RecvStream>,
        >,
    >,
) -> Result<(Won<PRes, RRes, NextHandler>, CursorCredit<State, Role, C>), Error<C>> {
    match Role::as_enum() {
        role::Role::Client => {
            trace!("as client");
            processor_transition.set_in_tiebreak(true);
            let ((res, next_handler), processor_transition) = processor_transition
                .reply()
                .await
                .map_err(write::Error::Send)?;
            let connection = processor_transition.into_conn();
            trace!("notifying");

            notify::send::<ProcessorSacrificed, _>((), &connection).await?;
            trace!("notified");
            Ok((
                Won::Processor { res, next_handler },
                CursorCredit {
                    connection,
                    _marker: PhantomData,
                },
            ))
        }
        role::Role::Server => {
            trace!("as server");
            let (res, requester_transition) = recv_fut.await?;
            let requester_transition: RequesterTransition<State, Role, C, Finished> =
                requester_transition;
            assert!(res.in_tiebreak);
            let connection = requester_transition.into_conn();
            trace!("waiting for notification");
            notify::receive::<ProcessorSacrificed, _>(&mut Vec::new(), &connection).await?;
            trace!("received notification");

            Ok((
                Won::Requester { res: res.reply },
                CursorCredit {
                    connection,
                    _marker: PhantomData,
                },
            ))
        }
    }
}

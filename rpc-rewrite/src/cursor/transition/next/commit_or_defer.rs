use crate::{
    cursor::{
        processor::{self, transition::delayed_replier::TransitionReply},
        requester::transition::{RequesterTransition, requester_transition::Finished},
        role,
        transition::{CursorCredit, Won, next::ProcessorSacrificed},
    },
    io::{Connection, notify, read, write},
};

use std::{marker::PhantomData, pin::pin, task::Poll};
use tracing::{instrument, trace};

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
    #[error("peer rejected our transition request before we replied to theirs")]
    UnexpectedRejection,
}

pub type CommitOrDeferResult<PRes, RRes, NextHandler, State, Role, C> =
    Result<(Won<PRes, RRes, NextHandler>, CursorCredit<State, Role, C>), Error<C>>;

/// Both sides have sent a transition request and our processor has finished
/// handling the peer's request (its reply is held, not yet sent).
///
/// Whichever side sends its held reply commits to the peer's request:
/// - If the reply to our own request already arrived, the peer committed
///   first, so we defer: discard our held reply and reject the peer's request.
/// - Otherwise we commit by sending our reply. If the peer deferred it rejects
///   our request; if the peer also committed it's a tie and the server's
///   request wins.
///
/// Every side that commits sends [`ProcessorSacrificed`], and every side
/// whose request was replied to receives exactly one.
#[instrument(skip_all, fields(role = ?Role::as_enum()))]
pub(super) async fn commit_or_defer_fn<
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
                Option<TransitionReply<RRes>>,
                RequesterTransition<State, Role, C, Finished>,
            ),
            crate::io::read::Error<<C as Connection>::RecvStream>,
        >,
    >,
) -> CommitOrDeferResult<PRes, RRes, NextHandler, State, Role, C> {
    let mut recv_fut = pin!(recv_fut);
    if let Poll::Ready(recv) = futures::poll!(recv_fut.as_mut()) {
        trace!("reply to our request arrived first; deferring");
        let (reply, requester_transition) = recv?;
        let reply = reply.ok_or(Error::UnexpectedRejection)?;
        processor_transition
            .reject()
            .await
            .map_err(write::Error::Send)?;
        let connection = requester_transition.into_conn();
        notify::receive::<ProcessorSacrificed, _>(&mut Vec::new(), &connection).await?;
        return Ok((
            Won::Requester { res: reply.reply },
            CursorCredit {
                connection,
                _marker: PhantomData,
            },
        ));
    }

    trace!("committing");
    processor_transition.set_in_tiebreak(true);
    let ((res, next_handler), processor_transition) = processor_transition
        .reply()
        .await
        .map_err(write::Error::Send)?;
    let connection = processor_transition.into_conn();
    notify::send::<ProcessorSacrificed, _>((), &connection).await?;

    trace!("waiting for the peer's decision on our request");
    let (reply, _requester_transition) = recv_fut.await?;
    let Some(reply) = reply else {
        trace!("peer deferred");
        return Ok((
            Won::Processor { res, next_handler },
            CursorCredit {
                connection,
                _marker: PhantomData,
            },
        ));
    };

    trace!("peer also committed; tie");
    notify::receive::<ProcessorSacrificed, _>(&mut Vec::new(), &connection).await?;
    let won = match Role::as_enum() {
        role::Role::Client => Won::Processor { res, next_handler },
        role::Role::Server => Won::Requester { res: reply.reply },
    };
    Ok((
        won,
        CursorCredit {
            connection,
            _marker: PhantomData,
        },
    ))
}

use std::{future::pending, pin::pin};

use futures::future::{Either, select};
use tokio::sync::{Notify as Wake, oneshot};
use tracing::{debug, trace};

use super::super::states::{
    Entrypoint, InRoom,
    in_room::{
        Notify, close,
        from_client::{self, leave, post},
    },
};
use rpc_rewrite::{
    cursor::{
        Cursor,
        requester::transition::RequestConcurrentTransitionError,
        state::{Wrapper, WrapperCredit},
        transition::{RequesterOrRequesterTransition, Won, tiebreak},
    },
    marker,
    method::{LeafHandler, ReqOf, ResOf, TransitionLeafHandler, handler::can_transition},
};

use super::rooms::{Event, Handle, Membership};

/// What a won client request asks us to do to the room. Applied only after
/// the transition is decided, because the leaf handlers below may be dropped
/// or have their reply discarded when the client's request loses a race.
pub enum Outcome {
    Leave,
    Close,
}

/// Server handler for everything the client can send in `InRoom`. `Clone`d
/// per accepted stream, so all state lives behind the [`Handle`].
#[derive(Clone)]
pub struct FromClient {
    handle: Handle,
}

struct PostHandler(Handle);

impl LeafHandler<post::Method> for PostHandler {
    /// Queues the message for every user in the room, including the poster.
    ///
    /// Differs from the old example: the reply does not wait for the
    /// notifications to be acked by every user (they are queued on each
    /// user's connection task, which awaits the acks). A user that fails to
    /// ack is removed from the room by its connection task.
    async fn handle<'a>(&mut self, body: ReqOf<'a, post::Method>) -> ResOf<'a, post::Method> {
        self.0.post(body);
    }
}

struct LeaveHandler;

impl TransitionLeafHandler<leave::Method> for LeaveHandler {
    type NextHandler = Outcome;

    async fn handle_transition<'a>(
        self,
        (): ReqOf<'a, leave::Method>,
        wrapper_credit: WrapperCredit<leave::Method>,
    ) -> (ResOf<'a, leave::Method>, Self::NextHandler) {
        (wrapper_credit.into(), Outcome::Leave)
    }
}

struct CloseHandler;

impl TransitionLeafHandler<close::Method> for CloseHandler {
    type NextHandler = Outcome;

    async fn handle_transition<'a>(
        self,
        (): ReqOf<'a, close::Method>,
        wrapper_credit: WrapperCredit<close::Method>,
    ) -> (ResOf<'a, close::Method>, Self::NextHandler) {
        (wrapper_credit.into(), Outcome::Close)
    }
}

impl can_transition::BranchHandler<from_client::Method> for FromClient {
    type NextHandler = Outcome;

    async fn handle_possible_transition<
        'a,
        R: rpc_rewrite::method::Replier<from_client::Method>
            + rpc_rewrite::method::replier::loopback::Replier<from_client::Method>
            + rpc_rewrite::method::replier::transition::Replier<from_client::Method>,
    >(
        self,
        request: ReqOf<'a, from_client::Method>,
        replier: R,
    ) -> can_transition::PossibleTransitionResult<'a, R, from_client::Method, Outcome> {
        Ok(match request {
            from_client::Req::Loopback(from_client::loopback::Req::Post(body)) => (
                replier
                    .reply_with_leaf::<post::Method, _>(body, &mut PostHandler(self.handle))
                    .await?,
                None,
            ),
            from_client::Req::Leave(()) => {
                let (receipt, next) = replier
                    .transition_with_leaf::<leave::Method, _>((), LeaveHandler)
                    .await?;
                (receipt, Some(next))
            }
            from_client::Req::Close(()) => {
                let (receipt, next) = replier
                    .transition_with_leaf::<close::Method, _>((), CloseHandler)
                    .await?;
                (receipt, Some(next))
            }
        })
    }
}

enum Next {
    /// the processor won: give the requester back
    HandBack,
    /// the room was closed: kick the client
    Kick,
}

/// Runs one user's time in a room. Resolves to the cursor at `Entrypoint`
/// after a Leave, a Close (by this client), or a kick (room closed by
/// someone else). The caller drops `membership` afterwards, which removes the
/// user from the room if that did not already happen.
///
/// The requester side owns the connection's `Requester`: it forwards queued
/// [`Event`]s to the client one at a time, awaiting each ack, and only looks at
/// `need_requester` between them so a `request_loopback` is never cancelled
/// (which would make the following transition fail with `AbandonedLoopback`).
pub async fn in_room(
    cursor: Cursor<InRoom, marker::Server, quinn::Connection>,
    membership: &mut Membership,
) -> anyhow::Result<Cursor<Entrypoint, marker::Server, quinn::Connection>> {
    let handle = membership.handle.clone();
    let rx = &mut membership.rx;
    let (processor, requester) = cursor.into_processor_and_requester(FromClient {
        handle: handle.clone(),
    });

    let mut buf = Vec::new();
    let mut processor_fut = pin!(processor.handle_hybrid_concurrent_transition_requests(&mut buf));
    let mut ack_buf = Vec::new();
    let mut read_into = Vec::new();
    let need_requester = Wake::new();
    let to_requester = oneshot::channel();
    let requester_fut = async {
        let mut idle = false;
        let next = loop {
            if idle {
                // nothing more to send (a notify failed or the room forgot
                // us); just wait for our processor to win
                need_requester.notified().await;
                break Next::HandBack;
            }
            match select(pin!(need_requester.notified()), pin!(rx.recv())).await {
                Either::Left(_) => break Next::HandBack,
                Either::Right((Some(Event::Notify(notification)), _)) => {
                    // never dropped mid-flight: see the function docs
                    if let Err(error) = requester
                        .request_loopback::<Notify>(notification, &mut ack_buf)
                        .await
                    {
                        // Either the peer is gone or it left/closed while
                        // this was in flight (a lost ack). Removal is
                        // idempotent, so both are fine.
                        debug!("notify failed: {error:?}");
                        handle.remove();
                        idle = true;
                    }
                }
                Either::Right((Some(Event::Kick), _)) => break Next::Kick,
                Either::Right((None, _)) => idle = true,
            }
        };
        match next {
            Next::HandBack => {
                trace!("handing requester back");
                to_requester.0.send(requester).ok().unwrap();
                pending().await
            }
            Next::Kick => Ok::<_, RequestConcurrentTransitionError<quinn::Connection>>(
                requester
                    .request_concurrent_transition::<close::Method>((), &mut read_into)
                    .await?,
            ),
        }
    };
    let mut requester_fut = pin!(requester_fut);
    let (won, credit) = match tiebreak(&mut processor_fut, &mut requester_fut).await? {
        tiebreak::With::Processor(processor_transition) => {
            need_requester.notify_one();
            processor_transition
                .next(async move {
                    let res = match select(requester_fut, to_requester.1).await {
                        Either::Left((requester_transition, _)) => {
                            RequesterOrRequesterTransition::RequesterTransition(
                                requester_transition?,
                            )
                        }
                        Either::Right((requester, _)) => {
                            RequesterOrRequesterTransition::Requester(requester?)
                        }
                    };
                    anyhow::Ok(res)
                })
                .await?
        }
        tiebreak::With::Requester(requester_transition) => {
            requester_transition.next(processor_fut).await?
        }
    };
    let wrapper: Wrapper<Entrypoint> = match won {
        Won::Processor { res, next_handler } => {
            match next_handler {
                Outcome::Leave => handle.remove(),
                Outcome::Close => handle.close_room(),
            }
            match res {
                from_client::Res::Leave(wrapper) | from_client::Res::Close(wrapper) => wrapper,
                from_client::Res::Loopback(_) => unreachable!("a loopback cannot transition"),
            }
        }
        // the room was already closed by whoever closed it
        Won::Requester { res } => res,
    };
    Ok(Cursor::from_cursor_credit(credit, wrapper))
}

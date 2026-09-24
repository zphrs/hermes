use std::{
    marker::PhantomData,
    pin::{Pin, pin},
};

use futures::future::select;
use tracing::{instrument, trace};

use crate::{
    cursor::{
        processor::{self, ProcessorFut, transition::delayed_replier::TransitionReply},
        role,
        transition::{
            CursorCredit, NextError, Won,
            next::{ProcessorSacrificed, commit_or_defer},
            requester_transition,
        },
    },
    io::notify,
    method::{Method, ResOf},
};

pub type Result<'rbuf, State, Role, C, PRes, RequesterMethod, NextHandler, ProcessorError> =
    std::result::Result<
        (
            Won<PRes, ResOf<'rbuf, RequesterMethod>, NextHandler>,
            CursorCredit<State, Role, C>,
        ),
        NextError<C, ProcessorError>,
    >;

/// race between requester sending a transition request and processor getting a transition
/// request.
#[instrument(skip_all)]
#[expect(private_bounds, reason = "role")]
pub async fn with_requester_transition<
    'pbuf,
    'rbuf,
    Role: role::Sealed,
    State,
    C: crate::io::Connection,
    PRes,
    NextHandler,
    RequesterRootRequest,
    RequesterMethod: Method,
    Fut,
    ProcessorError,
>(
    requester: requester_transition::Entrypoint<
        'rbuf,
        State,
        Role,
        C,
        RequesterRootRequest,
        RequesterMethod,
    >,
    processor: Pin<&mut ProcessorFut<Fut>>,
) -> Result<'rbuf, State, Role, C, PRes, RequesterMethod, NextHandler, ProcessorError>
where
    ResOf<'rbuf, RequesterMethod>: minicbor::Decode<'rbuf, ()>,
    Fut: Future<
        Output = std::result::Result<
            processor::transition::Entrypoint<State, Role, C, PRes, NextHandler>,
            ProcessorError,
        >,
    >,
{
    let recv = pin!(requester.receive());

    /// makes sure the ProcessorFut is cancelled before we return from the
    /// function
    struct CancelProcessorOnDrop<'a, Fut>(Pin<&'a mut ProcessorFut<Fut>>);

    impl<Fut> Drop for CancelProcessorOnDrop<'_, Fut> {
        fn drop(&mut self) {
            self.0.as_mut().cancel();
        }
    }

    let mut processor = CancelProcessorOnDrop(processor);

    match select(recv, processor.0.as_mut()).await {
        futures::future::Either::Left((recv_result, _)) => {
            let (reply, requester_transition) = recv_result?;
            // the peer can only reject our request after we replied to theirs
            let TransitionReply { reply, in_tiebreak } =
                reply.ok_or(NextError::UnexpectedRejection)?;
            if in_tiebreak {
                trace!("peer committed while it had a pending request; rejecting it");
                // the peer's request must be consumed by this state's processor
                // rather than leak into the next state's processor
                processor
                    .0
                    .as_mut()
                    .reject_transition()
                    .await
                    .map_err(NextError::Fut)?;
            } else {
                processor.0.as_mut().cancel();
            }
            let connection = requester_transition.into_conn();
            notify::receive::<ProcessorSacrificed, _>(&mut Vec::new(), &connection).await?;
            Ok((
                Won::Requester { res: reply },
                CursorCredit {
                    _marker: PhantomData,
                    connection,
                },
            ))
        }
        futures::future::Either::Right((processor_transition, recv_fut)) => {
            trace!("both sides requested a transition");
            let processor_transition = processor_transition.map_err(NextError::Fut)?;
            let res = commit_or_defer::commit_or_defer_fn(processor_transition, recv_fut).await?;
            Ok(res)
        }
    }
}

use std::{
    marker::PhantomData,
    pin::{Pin, pin},
};

use futures::future::{join, select};
use tracing::{instrument, trace};

use crate::{
    cursor::{
        processor::{self, ProcessorFut, transition::delayed_replier::TransitionReply},
        role,
        transition::{
            NextError, SharedCredit, Won,
            next::{
                ProcessorSacrificed,
                definite_tiebreak::{self},
            },
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
            SharedCredit<State, Role, C>,
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
    for<'a> ResOf<'a, RequesterMethod>: minicbor::Decode<'a, ()>,
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

    match select(recv, &mut processor.0).await {
        futures::future::Either::Left((recv_result, processor_fut)) => {
            let (TransitionReply { reply, in_tiebreak }, requester_transition) = recv_result?;
            let connection = requester_transition.into_conn();
            let mut read_buf = Vec::new();
            let notify_receive =
                notify::receive::<ProcessorSacrificed, _>(&mut read_buf, &connection);
            if in_tiebreak {
                trace!("in tiebreak; waiting for notification");
                // could get here if the remote approved our transition request
                // after it sent out a transition request of its own and tiebroke
                // between its local transition request and the one it sent to
                // us before it received ours.
                //
                // Thus we must wait for the remote's transition request to arrive
                // before we continue.

                let (_processor_transition, notify_res) = join(processor_fut, notify_receive).await;
                let () = notify_res?;
            } else {
                trace!("not in tiebreak");
                let () = notify_receive.await?;
            }
            Ok((
                Won::Requester { res: reply },
                SharedCredit {
                    _marker: PhantomData,
                    connection,
                },
            ))
        }
        futures::future::Either::Right((processor_transition, recv_fut)) => {
            trace!("running definite tiebreak");
            let processor_transition = processor_transition.map_err(NextError::Fut)?;
            let res =
                definite_tiebreak::definite_tiebreak_fn(processor_transition, recv_fut).await?;
            Ok(res)
        }
    }
}

use super::super::{
    NextError, ProcessorTransitionEntrypoint, RequesterOrRequesterTransition, SharedCredit, Won,
    next::ProcessorSacrificed,
};
use crate::{
    io::{
        notify::{self},
        write,
    },
    traits::{Method, method::ResOf, role},
};

use std::{marker::PhantomData, pin::pin};
use tracing::{instrument, trace};

pub type Result<'rbuf, State, Role, C, PRes, NextHandler, RequesterMethod, RequesterError> =
    std::result::Result<
        (
            Won<PRes, ResOf<'rbuf, RequesterMethod>, NextHandler>,
            SharedCredit<State, Role, C>,
        ),
        NextError<C, RequesterError>,
    >;

#[instrument(skip_all)]
#[expect(private_bounds, reason = "role")]
pub async fn with_processor_transition<
    'rbuf,
    'rreq,
    Role: role::Sealed,
    State,
    C: crate::io::Connection,
    PRes,
    NextHandler,
    RequesterRootMethod: Method,
    RequesterMethod: Method,
    RequesterError,
>(
    processor: ProcessorTransitionEntrypoint<State, Role, C, PRes, NextHandler>,
    requester: impl Future<
        Output = std::result::Result<
            RequesterOrRequesterTransition<
                'rbuf,
                'rreq,
                State,
                Role,
                RequesterRootMethod,
                C,
                RequesterMethod,
            >,
            RequesterError,
        >,
    >,
) -> Result<'rbuf, State, Role, C, PRes, NextHandler, RequesterMethod, RequesterError>
where
    ResOf<'rbuf, RequesterMethod>: minicbor::Decode<'rbuf, ()>,
{
    let res = requester.await.map_err(NextError::Fut)?;
    match res {
        RequesterOrRequesterTransition::Requester(requester) => {
            trace!("just transitioning; no tiebreak");
            // we're good to just transition; no tiebreak
            assert!(
                requester.conn().stable_id() == processor.conn().stable_id(),
                "requester and processor should both belong to the same connection"
            );
            let ((res, next_handler), processor_transition) =
                processor.reply().await.map_err(write::Error::Send)?;
            let connection = processor_transition.into_conn();
            // still need to notify that we've sacrificed our processor
            // because there's no way for the other side to know when our
            // old processor has stopped handling incoming requests.
            //
            // In theory this could be removed if it is known that our processor
            // is NotApplicable.
            notify::send::<ProcessorSacrificed, _>((), &connection).await?;

            Ok((
                Won::Processor { res, next_handler },
                SharedCredit {
                    connection,
                    _marker: PhantomData,
                },
            ))
        }
        RequesterOrRequesterTransition::RequesterTransition(requester_transition) => {
            assert!(
                requester_transition.conn().stable_id() == processor.conn().stable_id(),
                "requester and processor should both belong to the same connection"
            );
            trace!("tiebreaking");
            // need to tiebreak
            let _processor_res = processor.res();
            let _requester_res = requester_transition.res();

            let pinned_recv = pin!(requester_transition.receive());
            let res =
                super::definite_tiebreak::definite_tiebreak_fn(processor, pinned_recv).await?;
            Ok(res)
        }
    }
}

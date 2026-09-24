use super::super::{
    CursorCredit, NextError, RequesterOrRequesterTransition, Won, next::ProcessorSacrificed,
};
use crate::{
    cursor::{processor, role},
    io::{
        notify::{self},
        write,
    },
    method::{Method, ResOf},
};

use std::marker::PhantomData;
use tracing::{instrument, trace};

pub type Result<'rbuf, State, Role, C, PRes, NextHandler, RequesterMethod, RequesterError> =
    std::result::Result<
        (
            Won<PRes, ResOf<'rbuf, RequesterMethod>, NextHandler>,
            CursorCredit<State, Role, C>,
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
    processor: processor::transition::Entrypoint<State, Role, C, PRes, NextHandler>,
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
                CursorCredit {
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
            trace!("both sides requested a transition");
            let res = super::commit_or_defer::commit_or_defer_fn(
                processor,
                requester_transition.receive(),
            )
            .await?;
            Ok(res)
        }
    }
}

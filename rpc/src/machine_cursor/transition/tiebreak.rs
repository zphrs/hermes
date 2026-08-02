use std::pin::Pin;
use std::{convert::Infallible, pin::pin};

use futures::{FutureExt, select};
use tracing::{debug, warn};

use crate::{
    CallerError, MachineCursor,
    machine_cursor::{
        TransitionRequestError,
        transition::{
            RequestTransition,
            processor::{DelayedReplier, Entrypoint, FinalizeFuture, ProcessorTransition},
            requester::{
                AssertSacrificeError, RequesterTransition, ToSacrifice, TransitionReceipt,
                assert_remote_sacrifice,
            },
            transition_request_method,
        },
    },
    state::{self, PrioritizedUnsafeExt},
    transport::{ReplyHelper, ext::query_owned},
};

#[expect(private_bounds, reason = "for role")]
pub enum TiebreakResult<
    ProcessorMethod: crate::Method,
    RequesterRes,
    Role: crate::state::Role,
    Conn: crate::transport::Connection,
> {
    ProcessorWon(FinalizeProcessorTransition<ProcessorMethod::Res, Role, Conn>),
    RequesterWon(FinalizeRequesterTransition<RequesterRes, Role, Conn>),
}

pub struct FinalizeProcessorTransition<Res, Role, Conn: crate::transport::Connection> {
    res: Res,
    finalize_fut: FinalizeFuture<Conn::SendStream>,
    role: Role,
    conn: Conn,
}

impl<Res, Role, Conn: crate::transport::Connection> FinalizeProcessorTransition<Res, Role, Conn> {
    pub fn extract_res(self) -> (Res, FinalizeProcessorTransition<(), Role, Conn>) {
        (
            self.res,
            FinalizeProcessorTransition {
                res: (),
                finalize_fut: self.finalize_fut,
                role: self.role,
                conn: self.conn,
            },
        )
    }
}

#[expect(private_bounds, reason = "for role")]
impl<Role: crate::state::Role, Conn: crate::transport::Connection>
    FinalizeProcessorTransition<(), Role, Conn>
{
    pub async fn finish<State: crate::State>(
        mut self,
        wrapper: state::Wrapper<State>,
    ) -> Result<
        MachineCursor<State, Conn, Role>,
        AssertSacrificeError<<Conn as crate::transport::Client>::Error>,
    > {
        self.finalize_fut.await?;
        assert_remote_sacrifice(&mut self.conn).await?;
        Ok(MachineCursor::new_with_role(self.conn, self.role, wrapper))
    }
}

#[expect(private_bounds, reason = "for role")]
pub struct FinalizeRequesterTransition<
    TransitionRes,
    Role: crate::state::Role,
    Conn: crate::transport::Connection,
> {
    receipt: TransitionReceipt<TransitionRes, Role, Conn>,
    to_sacrifice: ToSacrifice,
}

#[expect(private_bounds, reason = "for role")]
impl<Res, Role: crate::state::Role, Conn: crate::transport::Connection>
    FinalizeRequesterTransition<Res, Role, Conn>
{
    pub fn extract_res(self) -> (Res, FinalizeRequesterTransition<(), Role, Conn>) {
        let (res, receipt) = self.receipt.extract_result();
        (
            res,
            FinalizeRequesterTransition {
                receipt,
                to_sacrifice: self.to_sacrifice,
            },
        )
    }
}

#[expect(private_bounds, reason = "for role")]
impl<Role: crate::state::Role, Conn: crate::transport::Connection>
    FinalizeRequesterTransition<(), Role, Conn>
{
    pub async fn finish<State: crate::state::State>(
        self,
        wrapper: state::Wrapper<State>,
    ) -> Result<MachineCursor<State, Conn, Role>, CallerError<<Conn as crate::Caller>::Error>> {
        let (role, conn) = self.receipt.into_parts(self.to_sacrifice).await?;
        Ok(MachineCursor::new_with_role(conn, role, wrapper))
    }
}

#[derive(thiserror::Error)]
pub enum TiebreakError<Conn: crate::transport::Connection, HandlerError> {
    #[error("minicbor: {0}")]
    Minicbor(#[from] minicbor_io::Error),
    #[error("caller: {0}")]
    Caller(#[from] CallerError<<Conn as crate::Caller>::Error>),
    #[error("transition request error: {0}")]
    TransitionRequest(
        #[from]
        TransitionRequestError<
            <Conn as crate::transport::Client>::Error,
            HandlerError,
            minicbor::encode::Error<Infallible>,
        >,
    ),
}

impl<Conn: crate::transport::Connection, HandlerError: std::fmt::Debug> std::fmt::Debug
    for TiebreakError<Conn, HandlerError>
where
    <Conn as crate::Caller>::Error: std::fmt::Debug,
    <Conn as crate::transport::Client>::Error: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Minicbor(arg0) => f.debug_tuple("Minicbor").field(arg0).finish(),
            Self::Caller(arg0) => f.debug_tuple("Caller").field(arg0).finish(),
            Self::TransitionRequest(arg0) => {
                f.debug_tuple("TransitionRequest").field(arg0).finish()
            }
        }
    }
}

#[derive(PartialEq)]
enum TiebreakChoice {
    Processor,
    Requester,
}

fn tiebreak_choice<Role: crate::state::Role, State: crate::state::Prioritized>(
    processor_priority: State::Priority,
    requester_priority: State::Priority,
) -> TiebreakChoice {
    // need to flip the request in order to break ties in a deterministic
    // direction, based on the one who initiated the connection
    match Role::to_enum() {
        state::role::WhichRole::Client => {
            match <State::Priority as crate::state::Priority<State>>::choose(
                processor_priority,
                requester_priority,
            ) {
                state::priority::Client => TiebreakChoice::Processor,
                state::priority::Server => TiebreakChoice::Requester,
            }
        }
        state::role::WhichRole::Server => {
            match <State::Priority as crate::state::Priority<State>>::choose(
                requester_priority,
                processor_priority,
            ) {
                state::priority::Client => TiebreakChoice::Requester,
                state::priority::Server => TiebreakChoice::Processor,
            }
        }
    }
}

#[expect(private_bounds, reason = "for role")]
pub async fn between_processor_and_requester_transition<
    'a,
    State: crate::state::Prioritized,
    ProcessorMethod: crate::Method,
    RootReq,
    TransitionMethod: crate::Method,
    Role: crate::state::Role,
    Conn: crate::transport::Connection,
>(
    processor_transition: ProcessorTransition<
        super::processor::Entrypoint<State, ProcessorMethod, Role, Conn>,
    >,
    requester_transition: RequesterTransition<
        State,
        RequestTransition<RootReq, TransitionMethod, Role, Conn>,
    >,
    to_sacrifice: ToSacrifice,
) -> EventualTiebreakResult<
    Result<
        TiebreakResult<ProcessorMethod, TransitionMethod::Res, Role, Conn>,
        TiebreakError<Conn, ()>,
    >,
    impl Future<
        Output = Result<
            TiebreakResult<ProcessorMethod, TransitionMethod::Res, Role, Conn>,
            TiebreakError<Conn, ()>,
        >,
    >,
>
where
    TransitionMethod::Res: crate::RpcMessage,
    RootReq: crate::RpcMessage + From<TransitionMethod::Req>,
{
    let request_transition = requester_transition.into_inner();

    let processor_transition_ref = processor_transition.inner_mut().inner_mut();

    assert!(
        request_transition.query_req().caller().unwrap() == processor_transition_ref.conn(),
        "processor and requester must both belong to the same connection"
    );
    let query_req = request_transition.query_req();
    let root_req = query_req.root_req().unwrap();

    let requester_priority = unsafe { State::requester_priority::<Role, _>(root_req) };

    let choice = tiebreak_choice::<Role, State>(
        processor_transition_ref.take_priority().unwrap(),
        requester_priority,
    );
    let res = {
        use TiebreakChoice::*;
        match choice {
            // processor won
            Processor => {
                let (delayed_receipt, role, conn, _wrapper, _processor_priority, sender) =
                    processor_transition.into_inner().into_inner().into_parts();
                debug!("processor won");
                // not in a tiebreak because we won and so we won't actually
                // send off the request transition (it's pending until we await
                // request_transition/call requester_transition.next())
                let (res, finalize_fut) = delayed_receipt.finalize(sender, false);
                TiebreakResult::ProcessorWon(FinalizeProcessorTransition {
                    res,
                    finalize_fut,
                    role,
                    conn,
                })
            }
            // requester won; need to wait for remote to come to the same
            // conclusion
            Requester => {
                debug!("requester won");
                let res = request_transition
                    .await
                    .map_err(|e| CallerError::try_from(e).unwrap())?
                    .1;
                let (res, receipt) = res.extract_result();
                let (in_transition, res) = res.into_parts();
                debug_assert!(in_transition);

                let receipt = receipt.insert_result(res);

                TiebreakResult::RequesterWon(FinalizeRequesterTransition {
                    receipt,
                    to_sacrifice: processor_transition.into(),
                })
            }
        }
    };
    Ok(res)
}

#[expect(private_bounds, reason = "for role")]
pub async fn between_potential_processor_and_known_requester_transition<
    State: crate::state::Prioritized,
    ProcessorMethod: crate::Method,
    TransitionMethod: crate::Method,
    RootReq,
    Role: crate::state::Role,
    Conn: crate::transport::Connection,
    HError,
    ToProcessorTransition: Future<
        Output = Result<
            ProcessorTransition<super::processor::Entrypoint<State, ProcessorMethod, Role, Conn>>,
            TransitionRequestError<
                <Conn as crate::transport::Client>::Error,
                HError,
                <DelayedReplier<ProcessorMethod> as ReplyHelper<
                    ProcessorMethod,
                    ProcessorMethod,
                >>::Error,
            >,
        >,
    >,
>(
    to_processor_transition: ToProcessorTransition,
    to_sacrifice: ToSacrifice,
    requester_transition: RequesterTransition<
        State,
        RequestTransition<RootReq, TransitionMethod, Role, Conn>,
    >,
) -> Result<
    TiebreakResult<ProcessorMethod, TransitionMethod::Res, Role, Conn>,
    TiebreakError<Conn, HError>,
>
where
    <Conn as crate::transport::Client>::Error: std::fmt::Debug,
    HError: std::fmt::Debug,
    TransitionMethod::Res: crate::RpcMessage,
    RootReq: crate::RpcMessage + From<TransitionMethod::Req>,
    <Conn as crate::Caller>::Error: std::fmt::Debug,
{
    let fut = async move {
        let to_processor_transition = pin!(to_processor_transition);
        let mut request_transition = requester_transition.into_inner();
        let requester_priority = unsafe {
            State::requester_priority::<Role, _>(
                request_transition
                    .query_req()
                    .root_req()
                    .expect("should be defined because request_transition hasn't been awaited yet"),
            )
        };

        let mut to_processor_transition = to_processor_transition.fuse();

        enum Select<
            State: crate::state::Prioritized,
            RootMethod: crate::Method,
            RootReq,
            Role: crate::state::Role,
            Conn: crate::transport::Connection,
        > {
            Processor(ProcessorTransition<Entrypoint<State, RootMethod, Role, Conn>>),
            Requester(
                (
                    RootReq,
                    TransitionReceipt<transition_request_method::Res, Role, Conn>,
                ),
            ),
        }

        let result = select! {
            processor_transition = to_processor_transition => {
                let processor_transition = processor_transition?;
                Select::Processor(processor_transition)
            },
            tuple = request_transition => {
                Select::Requester(tuple.map_err(|e| CallerError::try_from(e).unwrap())?)
            },
        };
        let out = match result {
            Select::Processor(processor_transition) => {
                let mut processor_transition = processor_transition.into_inner().into_inner();
                debug!("processor finished first");
                // we know for sure we're tiebreaking here
                assert!(
                    processor_transition_mut.conn() == receipt.connection(),
                    "processor and requester must both belong to the same connection"
                );

                // tell other side we're tiebreaking
                match tiebreak_choice::<Role, State>(
                    processor_transition.take_priority().unwrap(),
                    requester_priority,
                ) {
                    TiebreakChoice::Processor => {
                        let (delayed_receipt, role, conn, _wrapper, _processor_priority, sender) =
                            processor_transition.into_parts();
                        debug!("processor won tiebreak");
                        let (res, finalize_fut) = delayed_receipt.finalize(sender, true);
                        // make sure we cancelled properly
                        assert!(matches!(
                            request_transition.into_inner().abort_early().await,
                            Err(query_owned::Error::Cancelled(_, _))
                        ));

                        TiebreakResult::<ProcessorMethod, TransitionMethod::Res, Role, Conn>::ProcessorWon(
                            FinalizeProcessorTransition {
                                res,
                                finalize_fut,
                                role,
                                conn,
                            },
                        )
                    }
                    TiebreakChoice::Requester => {
                        debug!("requester won tiebreak");
                        let (_root_req, receipt) = request_transition
                            .await
                            .map_err(|e| CallerError::try_from(e).unwrap())?;
                        let (res, receipt) = receipt.extract_result();
                        let (in_tiebreak, res): (_, TransitionMethod::Res) = res.into_parts();
                        assert!(in_tiebreak);
                        let receipt = receipt.insert_result(res);

                        TiebreakResult::RequesterWon(FinalizeRequesterTransition {
                            receipt,
                            to_sacrifice,
                        })
                    }
                }
            }
            Select::Requester((_root_req, receipt)) => {
                debug!("requester won first");
                let (res, receipt) = receipt.extract_result();
                // if we got here then it means the other side either:
                // - tiebroke in the requester's favor or
                // - didn't have any tiebreak whatsoever.
                // We need to figure out which one the other side did to ensure we
                // consume the request sent out by the other side.
                let (in_tiebreak, res): (_, TransitionMethod::Res) = res.into_parts();
                let receipt = receipt.insert_result(res);

                if in_tiebreak {
                    debug!("in tiebreak");
                    let processor_transition = to_processor_transition.await?;
                    let mut processor_transition = processor_transition.into_inner().into_inner();

                    assert!(
                        processor_transition.conn() == receipt.connection(),
                        "processor and requester must both belong to the same connection"
                    );
                    assert!(
                        tiebreak_choice::<Role, State>(
                            processor_transition.take_priority().unwrap(),
                            requester_priority
                        ) == TiebreakChoice::Requester
                    );

                    TiebreakResult::RequesterWon(FinalizeRequesterTransition {
                        receipt,
                        to_sacrifice,
                    })
                } else {
                    debug!("not in tiebreak");
                    warn!("NO ASSERTION THAT THE OLD PROCESSOR IS DROPPED!!");
                    TiebreakResult::RequesterWon(FinalizeRequesterTransition {
                        receipt,
                        to_sacrifice,
                    })
                }
            }
        };
        Ok(out)
    };
    Ok(out)
}

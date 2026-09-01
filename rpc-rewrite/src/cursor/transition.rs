use std::{marker::PhantomData, pin::pin};

use futures::{
    TryFutureExt,
    future::{join, select},
};
use tracing::{instrument, trace};

use crate::{
    cursor::{
        processor::{
            ProcessorFut,
            transitions::{
                delayed_replier::TransitionReply, processor_transition::ProcessorTransition,
            },
        },
        requester::transition::requester_transition::RecvError,
    },
    io::notify::{self, ReceiveError, SendError},
    traits::{
        self, Connection,
        io::BytesWriteStream,
        markers::{False, NotApplicable},
        method::{Method, ReqOf, ResOf},
        role,
    },
};

use super::{
    processor::transitions::processor_transition,
    requester::transition::{RequesterTransition, requester_transition},
};

type RequesterTransitionEntrypoint<'buf, State, Role, C, RootRequest, M> = RequesterTransition<
    State,
    Role,
    C,
    requester_transition::Sent<'buf, RootRequest, <C as traits::Connection>::RecvStream, M>,
>;

pub enum RequesterOrRequesterTransition<
    'buf,
    'req,
    State,
    Role,
    RootMethod: Method,
    C: traits::Connection,
    M,
> {
    Requester(super::requester::Requester<State, Role, RootMethod, C>),
    RequesterTransition(
        RequesterTransitionEntrypoint<'buf, State, Role, C, ReqOf<'req, RootMethod>, M>,
    ),
}

impl<'buf, 'req, State, Role, RootMethod: Method, C: traits::Connection>
    From<super::requester::Requester<State, Role, RootMethod, C>>
    for RequesterOrRequesterTransition<'buf, 'req, State, Role, RootMethod, C, NotApplicable>
{
    fn from(value: super::requester::Requester<State, Role, RootMethod, C>) -> Self {
        Self::Requester(value)
    }
}

impl<'buf, 'req, State, Role, RootMethod: Method, C: traits::Connection>
    RequesterOrRequesterTransition<'buf, 'req, State, Role, RootMethod, C, NotApplicable>
{
    pub async fn immediate_requester(
        requester: super::requester::Requester<State, Role, RootMethod, C>,
    ) -> Self {
        Self::Requester(requester)
    }
}

impl<'buf, 'req, State, Role, RootMethod: Method, C: traits::Connection, M>
    RequesterOrRequesterTransition<'buf, 'req, State, Role, RootMethod, C, M>
{
    pub fn conn(&self) -> &C {
        match self {
            RequesterOrRequesterTransition::Requester(requester) => requester.conn(),
            RequesterOrRequesterTransition::RequesterTransition(requester_transition) => {
                requester_transition.conn()
            }
        }
    }
}
type ProcessorTranstionEntrypoint<State, Role, C, Res, SendStream, NextHandler> =
    ProcessorTransition<
        State,
        Role,
        C,
        processor_transition::ReplyPrimed<Res, SendStream, NextHandler>,
    >;
pub async fn tiebreak<
    'rbuf,
    'rreq,
    State,
    Role,
    C: traits::Connection,
    Res,
    SendStream: BytesWriteStream,
    NextHandler,
    RootRequest,
    M,
    ProcessorError,
    RequesterError,
>(
    eventual_processor: &mut (
             impl Future<
        Output = Result<
            ProcessorTranstionEntrypoint<State, Role, C, Res, SendStream, NextHandler>,
            ProcessorError,
        >,
    > + Unpin
         ),
    eventual_requester: &mut (
             impl Future<
        Output = Result<
            RequesterTransitionEntrypoint<'rbuf, State, Role, C, RootRequest, M>,
            RequesterError,
        >,
    > + Unpin
         ),
) -> TiebreakResult<
    'rbuf,
    State,
    Role,
    C,
    Res,
    SendStream,
    NextHandler,
    RootRequest,
    M,
    ProcessorError,
    RequesterError,
> {
    match select(eventual_processor, eventual_requester).await {
        futures::future::Either::Left((processor_transition, _eventual_requester)) => {
            TiebreakResult::Processor(processor_transition)
        }
        futures::future::Either::Right((requester_transition, _eventual_processor)) => {
            TiebreakResult::Requester(requester_transition)
        }
    }
}

pub enum TiebreakResult<
    'rbuf,
    State,
    Role,
    C: traits::Connection,
    Res,
    SendStream: BytesWriteStream,
    NextHandler,
    RootRequest,
    M,
    ProcessorError,
    RequesterError,
> {
    Processor(
        Result<
            ProcessorTranstionEntrypoint<State, Role, C, Res, SendStream, NextHandler>,
            ProcessorError,
        >,
    ),
    Requester(
        Result<
            RequesterTransitionEntrypoint<'rbuf, State, Role, C, RootRequest, M>,
            RequesterError,
        >,
    ),
}

pub enum Won<PRes, RRes, NextHandler> {
    Processor {
        res: PRes,
        next_handler: NextHandler,
    },
    Requester {
        res: RRes,
    },
}

pub struct SharedCredit<State, Role, C> {
    _marker: PhantomData<(State, Role)>,
    connection: C,
    in_tiebreak: bool,
}

impl<State, Role, C> SharedCredit<State, Role, C> {
    pub fn into_connection(self) -> C {
        self.connection
    }
}

struct ProcessorSacrificed;

impl Method for ProcessorSacrificed {
    type Req<'buf> = ();

    type Res<'buf> = NotApplicable;

    type Transitions = False;

    type HasDescendants = False;
}

macro_rules! with_dollar_sign {
    ($($body:tt)*) => {
        macro_rules! __with_escape { $($body)* }
        __with_escape!($);
    }
}
/// inline_fn because compiler couldn't infer the type of [`recv_fut`] when called
macro_rules! inline_fn {
    ( $func:ident, $( $par_name:ident $(: $par_type:ty )?,)* $func_body:block) => {
        paste::paste! {
            with_dollar_sign! { ($d:tt) => {
                macro_rules! $func {
                    ( $( $par_name = $d [< $par_name _par_val >]:expr ),* ) => {{
                        $(
                            #[allow(unused_mut)]
                            let mut $par_name $(: $par_type)? = $d [< $par_name _par_val >];
                        )*
                        $func_body
                    }}
                }
            }}
        }
    };
}

inline_fn!(definite_tiebreak, processor, recv_fut, {
    // tiebreak logic goes here; for now we just default to server
    // request arbitrarily having priority
    async move {
        match Role::as_enum() {
            role::Role::Client => {
                trace!("as client");
                processor.set_in_tiebreak(true);
                let ((res, next_handler), processor_transition) = processor
                    .reply()
                    .await
                    .map_err(|e| DefiniteTiebreakError::Send(SendError::Write(e)))?;
                let connection = processor_transition.into_conn();
                trace!("notifying");

                notify::send::<ProcessorSacrificed, _>((), &connection).await?;
                trace!("notified");
                Ok((
                    Won::Processor { res, next_handler },
                    SharedCredit {
                        connection,
                        in_tiebreak: true,
                        _marker: PhantomData,
                    },
                ))
            }
            role::Role::Server => {
                trace!("as server");
                let (res, requester_transition) = recv_fut.await?;
                assert!(res.in_tiebreak);
                let connection = requester_transition.into_conn();
                trace!("waiting for notification");
                notify::receive::<ProcessorSacrificed, _>(&mut Vec::new(), &connection).await?;
                trace!("received notification");

                Ok((
                    Won::Requester { res: res.reply },
                    SharedCredit {
                        connection,
                        in_tiebreak: true,
                        _marker: PhantomData,
                    },
                ))
            }
        }
    }
});

#[derive(thiserror::Error)]
pub enum NextError<C: Connection, FutErr> {
    #[error("definite tiebreak: {0}")]
    DefiniteTiebreak(#[from] DefiniteTiebreakError<C>),
    #[error("error from future resolution: {0}")]
    PassedInFut(FutErr),
}

impl<C: Connection, R: std::fmt::Debug> std::fmt::Debug for NextError<C, R>
where
    DefiniteTiebreakError<C>: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DefiniteTiebreak(arg0) => f.debug_tuple("DefiniteTiebreak").field(arg0).finish(),
            Self::PassedInFut(arg0) => f.debug_tuple("PassedInFut").field(arg0).finish(),
        }
    }
}

#[instrument(skip_all)]
#[expect(private_bounds, reason = "role")]
pub async fn next_with_requester_transition<
    'pbuf,
    'rbuf,
    'rreq,
    Role: role::Sealed,
    State,
    C: traits::Connection,
    PRes,
    NextHandler,
    RequesterRootRequest,
    RequesterMethod: Method,
    Fut,
    ProcessorError,
>(
    requester: RequesterTransitionEntrypoint<
        'rbuf,
        State,
        Role,
        C,
        RequesterRootRequest,
        RequesterMethod,
    >,
    processor: ProcessorFut<Fut>,
) -> Result<
    (
        Won<PRes, ResOf<'rbuf, RequesterMethod>, NextHandler>,
        SharedCredit<State, Role, C>,
    ),
    NextError<C, ProcessorError>,
>
where
    for<'a> ResOf<'a, RequesterMethod>: minicbor::Decode<'a, ()>,
    Fut: Future<
        Output = Result<
            ProcessorTransition<
                State,
                Role,
                C,
                processor_transition::ReplyPrimed<PRes, C::SendStream, NextHandler>,
            >,
            ProcessorError,
        >,
    >,
{
    // race between requester receiving and processor getting a transition
    // request.
    let recv = pin!(requester.recv());
    let processor = pin!(processor);

    match select(recv, processor).await {
        futures::future::Either::Left((recv_result, processor_fut)) => {
            let (TransitionReply { reply, in_tiebreak }, requester_transition) =
                recv_result.map_err(DefiniteTiebreakError::from)?;
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
                let () = notify_res.map_err(DefiniteTiebreakError::from)?;
            } else {
                trace!("not in tiebreak");
                let () = notify_receive.await.map_err(DefiniteTiebreakError::from)?;
            }
            Ok((
                Won::Requester { res: reply },
                SharedCredit {
                    _marker: PhantomData,
                    in_tiebreak,
                    connection,
                },
            ))
        }
        futures::future::Either::Right((processor_transition, recv_fut)) => {
            trace!("running definite tiebreak");
            let processor_transition = processor_transition.map_err(NextError::PassedInFut)?;
            definite_tiebreak!(processor = processor_transition, recv_fut = recv_fut)
                .map_err(NextError::DefiniteTiebreak)
                .await
        }
    }
}

#[instrument(skip_all)]
#[expect(private_bounds, reason = "role")]
pub async fn next_with_processor_transition<
    'rbuf,
    'rreq,
    Role: role::Sealed,
    State,
    C: traits::Connection,
    PRes,
    NextHandler,
    RequesterRootMethod: Method,
    RequesterMethod: Method,
    RequesterError,
>(
    processor: ProcessorTranstionEntrypoint<State, Role, C, PRes, C::SendStream, NextHandler>,
    requester: impl Future<
        Output = Result<
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
) -> Result<
    (
        Won<PRes, ResOf<'rbuf, RequesterMethod>, NextHandler>,
        SharedCredit<State, Role, C>,
    ),
    NextError<C, RequesterError>,
>
where
    ResOf<'rbuf, RequesterMethod>: minicbor::Decode<'rbuf, ()>,
{
    let res = requester.await.map_err(NextError::PassedInFut)?;
    match res {
        RequesterOrRequesterTransition::Requester(requester) => {
            trace!("just transitioning; no tiebreak");
            // we're good to just transition; no tiebreak
            assert!(
                requester.conn().stable_id() == processor.conn().stable_id(),
                "requester and processor should both belong to the same connection"
            );
            let ((res, next_handler), processor_transition) = processor
                .reply()
                .await
                .map_err(|e| DefiniteTiebreakError::Send(SendError::<C>::Write(e)))?;
            let connection = processor_transition.into_conn();
            // still need to notify that we've sacrificed our processor
            // because there's no way for the other side to know when our
            // old processor has stopped handling incoming requests.
            //
            // In theory this could be removed if it is known that our processor
            // is NotApplicable.
            notify::send::<ProcessorSacrificed, _>((), &connection)
                .await
                .map_err(|e| NextError::DefiniteTiebreak(e.into()))?;

            Ok((
                Won::Processor { res, next_handler },
                SharedCredit {
                    connection,
                    in_tiebreak: false,
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

            let pinned_recv = pin!(requester_transition.recv());
            let res = definite_tiebreak!(processor = processor, recv_fut = pinned_recv).await;
            res.map_err(NextError::DefiniteTiebreak)
        }
    }
}

#[derive(thiserror::Error)]
pub enum DefiniteTiebreakError<C: Connection> {
    #[error("recv: {0}")]
    Recv(#[from] RecvError<C::RecvStream>),
    #[error("receive: {0}")]
    Receive(#[from] ReceiveError<C>),
    #[error("send: {0}")]
    Send(#[from] SendError<C>),
}

impl<C: Connection> std::fmt::Debug for DefiniteTiebreakError<C>
where
    ReceiveError<C>: std::fmt::Debug,
    SendError<C>: std::fmt::Debug,
    RecvError<C::RecvStream>: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Recv(arg0) => f.debug_tuple("Recv").field(arg0).finish(),
            Self::Receive(arg0) => f.debug_tuple("Receive").field(arg0).finish(),
            Self::Send(arg0) => f.debug_tuple("Send").field(arg0).finish(),
        }
    }
}

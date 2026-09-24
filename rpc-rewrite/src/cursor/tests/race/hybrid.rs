//! [`race_once`](super::race_once) with both sides running the hybrid
//! processor. Each side makes a loopback request before racing to transition
//! so the losing transition request has to be picked out from among loopback
//! streams.

use std::{future::pending, net::SocketAddr, pin::pin, time::Duration};

use futures::future::select;
use hegel::{HealthCheck, TestCase};
use tokio::sync::oneshot;
use tracing::trace;

use super::{
    CLIENT_ELAPSED, CLIENT_REACHED, SERVER_REACHED, Winner, draw_lossy_config, draw_one_long,
    draw_one_long_handler, on_fresh_thread, run_race, symmetric_config,
};
use crate::{
    cursor::{
        Cursor,
        processor::Processor,
        requester::{
            RequestLoopbackError, Requester, transition::RequestConcurrentTransitionError,
        },
        role,
        state::{self, WrapperCredit},
        tests::{accept_client, connect_to_server},
        transition::{CursorCredit, RequesterOrRequesterTransition, Won, tiebreak},
    },
    marker::{self, NotApplicable, not_applicable},
    method::{
        Descendant, LeafHandler, ReqOf, ResOf, TransitionLeafHandler, handler::can_transition,
    },
};

pub struct Racing;

impl state::State for Racing {
    type ClientBranchType = marker::CanTransition;
    type ClientHandles = Root;

    type ServerBranchType = marker::CanTransition;
    type ServerHandles = Root;
}

impl state::Entrypoint for Racing {}

pub struct Raced;

impl state::State for Raced {
    type ClientBranchType = marker::Loopback;
    type ClientHandles = NotApplicable;

    type ServerBranchType = marker::Loopback;
    type ServerHandles = NotApplicable;
}

#[derive(Clone, Copy)]
pub struct Root;

impl crate::Method for Root {
    type Req<'buf> = RootRequest<'buf>;
    type Res<'buf> = RootResponse<'buf>;

    type Type = crate::method::BranchCanTransition;
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum RootRequest<'buf> {
    #[n(0)]
    Ping(#[n(0)] &'buf minicbor::bytes::ByteSlice),
    #[n(1)]
    Race(#[n(0)] ReqOf<'buf, Race>),
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum RootResponse<'buf> {
    #[n(0)]
    Ping(#[n(0)] &'buf minicbor::bytes::ByteSlice),
    #[n(1)]
    Race(#[n(0)] ResOf<'buf, Race>),
}

impl can_transition::BranchHandler for Root {
    type NextHandler = not_applicable::Handler;

    async fn handle_possible_transition<
        'a,
        R: crate::method::Replier<Self>
            + crate::method::replier::loopback::Replier<Self>
            + crate::method::replier::transition::Replier<Self>,
    >(
        self,
        request: ReqOf<'a, Self>,
        replier: R,
    ) -> Result<(R::Receipt<ResOf<'a, Self>>, Option<Self::NextHandler>), R::Error> {
        Ok(match request {
            RootRequest::Ping(request) => {
                (replier.reply_with_leaf(request, &mut Ping).await?, None)
            }
            RootRequest::Race(request) => {
                let (receipt, next_handler) = replier.transition_with_leaf(request, Race).await?;
                (receipt, Some(next_handler))
            }
        })
    }
}

pub struct Ping;

impl crate::Method for Ping {
    type Req<'buf> = &'buf minicbor::bytes::ByteSlice;
    type Res<'buf> = &'buf minicbor::bytes::ByteSlice;

    type Type = crate::method::LeafLoopback;
}

impl LeafHandler for Ping {
    async fn handle<'a>(&mut self, request: ReqOf<'a, Self>) -> ResOf<'a, Self> {
        request
    }
}

impl Descendant<Root> for Ping {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, Root> {
        RootRequest::Ping(req)
    }

    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, Root> {
        RootResponse::Ping(res)
    }
}

/// sleeps for the requested duration before approving the transition
pub struct Race;

impl crate::Method for Race {
    type Req<'buf> = Duration;
    type Res<'buf> = state::Wrapper<Raced>;

    type Type = crate::method::LeafTransition;
}

impl TransitionLeafHandler for Race {
    type NextHandler = not_applicable::Handler;

    async fn handle_transition<'a>(
        self,
        duration: ReqOf<'a, Self>,
        wrapper_credit: WrapperCredit<Self>,
    ) -> (ResOf<'a, Self>, Self::NextHandler) {
        tokio::time::sleep(duration).await;
        (wrapper_credit.into(), not_applicable::Handler)
    }
}

impl Descendant<Root> for Race {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, Root> {
        RootRequest::Race(req)
    }

    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, Root> {
        RootResponse::Race(res)
    }
}

#[derive(Debug, thiserror::Error)]
enum RequesterError {
    #[error(transparent)]
    Loopback(#[from] RequestLoopbackError<quinn::Connection>),
    #[error(transparent)]
    Transition(#[from] RequestConcurrentTransitionError<quinn::Connection>),
}

/// Pings the peer and then races the peer to transition, handling the peer's
/// requests with the hybrid processor throughout. Resolves to whether our
/// processor won (i.e. the peer's request won).
async fn race_side<Role: role::Sealed>(
    processor: Processor<Racing, Role, Root, quinn::Connection, Root>,
    requester: Requester<Racing, Role, Root, quinn::Connection>,
    (request, request_delay): (Duration, Duration),
) -> anyhow::Result<(
    bool,
    state::Wrapper<Raced>,
    CursorCredit<Racing, Role, quinn::Connection>,
)> {
    let mut buf = Vec::new();
    let mut processor_fut = pin!(processor.handle_hybrid_concurrent_transition_requests(&mut buf));
    let mut ping_buf = Vec::new();
    let mut read_into = Vec::new();
    let need_requester = tokio::sync::Notify::new();
    let to_requester = oneshot::channel();
    let requester_fut = async {
        let ping: &[u8] = b"ping";
        let pong = requester
            .request_loopback::<Ping>(ping.into(), &mut ping_buf)
            .await?;
        assert_eq!(&**pong, ping);
        match select(
            pin!(need_requester.notified()),
            pin!(tokio::time::sleep(request_delay)),
        )
        .await
        {
            futures::future::Either::Left(_) => {
                trace!("notified!");
                to_requester.0.send(requester).ok().unwrap();
                pending().await
            }
            futures::future::Either::Right(_) => Ok::<_, RequesterError>(
                requester
                    .request_concurrent_transition::<Race>(request, &mut read_into)
                    .await?,
            ),
        }
    };
    let mut requester_fut = pin!(requester_fut);
    let (won, credit) = match tiebreak(&mut processor_fut, &mut requester_fut).await? {
        tiebreak::With::Processor(processor_transition) => {
            trace!("processor won");
            need_requester.notify_one();
            processor_transition
                .next(async move {
                    let res = match select(requester_fut, to_requester.1).await {
                        futures::future::Either::Left((requester_transition, _)) => {
                            RequesterOrRequesterTransition::RequesterTransition(
                                requester_transition?,
                            )
                        }
                        futures::future::Either::Right((requester, _)) => {
                            RequesterOrRequesterTransition::Requester(requester?)
                        }
                    };
                    anyhow::Ok(res)
                })
                .await?
        }
        tiebreak::With::Requester(requester_transition) => {
            trace!("requester won");
            requester_transition.next(processor_fut).await?
        }
    };
    Ok(match won {
        Won::Processor { res, .. } => match res {
            RootResponse::Race(wrapper) => (true, wrapper, credit),
            RootResponse::Ping(_) => unreachable!("a ping cannot transition"),
        },
        Won::Requester { res } => (false, res, credit),
    })
}

async fn server(endpoint: quinn::Endpoint, durations: (Duration, Duration)) -> anyhow::Result<()> {
    let conn = accept_client(&endpoint).await?;
    let cursor: Cursor<Racing, marker::Server, _> = Cursor::new(conn);
    let (processor, requester) = cursor.into_processor_and_requester(Root);
    let (processor_won, raced, credit) = race_side(processor, requester, durations).await?;
    Cursor::from_cursor_credit(credit, raced)
        .wait_to_close()
        .await?;
    let winner = if processor_won {
        Winner::Client
    } else {
        Winner::Server
    };
    SERVER_REACHED.with(|sr| sr.lock().unwrap().replace(winner));
    Ok(())
}

async fn client(
    endpoint: quinn::Endpoint,
    server_addr: SocketAddr,
    durations: (Duration, Duration),
) -> anyhow::Result<()> {
    // the machine's clock is paused, so this measures sim-time
    let start = tokio::time::Instant::now();
    let conn = connect_to_server(&endpoint, server_addr).await?;
    let cursor: Cursor<Racing, marker::Client, _> = Cursor::new(conn);
    let (processor, requester) = cursor.into_processor_and_requester(Root);
    let (processor_won, raced, credit) = race_side(processor, requester, durations).await?;
    Cursor::from_cursor_credit(credit, raced).close().await?;
    let winner = if processor_won {
        Winner::Server
    } else {
        Winner::Client
    };
    CLIENT_REACHED.with(|cr| cr.lock().unwrap().replace(winner));
    CLIENT_ELAPSED.with(|e| e.lock().unwrap().replace(start.elapsed()));
    Ok(())
}

#[test_log::test]
#[hegel::test(test_cases = 8, report_multiple_failures = true)]
fn hybrid_race_once(tc: TestCase) {
    on_fresh_thread(tc, |tc| {
        let (client_durations, server_durations, long_duration) = draw_one_long(&tc);
        let (_winner, sim_elapsed) = run_race(
            move |a, b| client(a, b, client_durations),
            move |e| server(e, server_durations),
            None,
        );
        assert!(
            sim_elapsed < long_duration,
            "should not wait out the {long_duration:?} delay (waited {sim_elapsed:?} of sim-time)"
        );
    });
}

/// see [`tie`](super::tie)
#[test_log::test]
#[hegel::test]
#[ignore = "slow: ~37.4s"]
fn hybrid_tie(tc: TestCase) {
    run_race(
        |a, b| client(a, b, (Duration::ZERO, Duration::ZERO)),
        |e| server(e, (Duration::ZERO, Duration::ZERO)),
        symmetric_config(&tc),
    );
}

/// see [`race_once_lossy`](super::race_once_lossy)
#[test_log::test]
#[hegel::test(test_cases = 32, suppress_health_check = [HealthCheck::TooSlow])]
#[ignore = "slow: ~14.2s"]
fn hybrid_race_once_lossy(tc: TestCase) {
    on_fresh_thread(tc, |tc| {
        let (client_durations, server_durations, _) = draw_one_long_handler(&tc);
        run_race(
            move |a, b| client(a, b, client_durations),
            move |e| server(e, server_durations),
            draw_lossy_config(&tc),
        );
    });
}

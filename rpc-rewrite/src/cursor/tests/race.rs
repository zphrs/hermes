mod hybrid;
pub mod states;

use std::{future::pending, net::SocketAddr, pin::pin, sync::Mutex, time::Duration};

use dens::sim::{
    Config,
    config::{Latency, MessageLoss},
};
use futures::future::select;
use hegel::{HealthCheck, TestCase, generators as gs};
use quinn::Endpoint;
use scoped_tls::scoped_thread_local;
use tokio::sync::oneshot;
use tracing::trace;

use crate::{
    cursor::{
        Cursor,
        state::{self, WrapperCredit},
        tests::{
            harness,
            race::states::entrypoint::{self, ClientRequestWins, ServerRequestWins},
        },
        transition::{RequesterOrRequesterTransition, tiebreak},
    },
    marker::{Client, Server, not_applicable},
    method::{
        self,
        handler::{TransitionLeafHandler, root_method::RootHandler},
    },
};

#[derive(PartialEq, Debug)]
enum Winner {
    Client,
    Server,
}

pub struct ServerHandler;

impl TransitionLeafHandler<entrypoint::ClientRequestWins> for ServerHandler {
    type NextHandler = not_applicable::Handler;

    async fn handle_transition<'a>(
        self,
        duration: method::ReqOf<'a, entrypoint::ClientRequestWins>,
        wrapper_credit: WrapperCredit<entrypoint::ClientRequestWins>,
    ) -> (
        method::ResOf<'a, entrypoint::ClientRequestWins>,
        Self::NextHandler,
    ) {
        tokio::time::sleep(duration).await;
        (
            state::Wrapper::from_wrapper_credit(wrapper_credit),
            not_applicable::Handler,
        )
    }
}

async fn server(
    endpoint: Endpoint,
    (request, request_delay): (Duration, Duration),
) -> anyhow::Result<()> {
    tracing::trace!("running");
    let conn = super::accept_client(&endpoint).await?;

    let e_cursor: Cursor<states::entrypoint::State, Server, _> = Cursor::new(conn);

    let (processor, requester) = e_cursor.into_processor_and_requester(RootHandler(ServerHandler));
    let mut buf = Vec::new();
    let mut processor_fut = pin!(processor.handle_concurrent_transition_request(&mut buf));
    let mut read_into = Vec::new();
    let need_requester = tokio::sync::Notify::new();
    let to_requester = oneshot::channel();
    let requester_fut = async {
        match select(
            pin!(need_requester.notified()),
            pin!(tokio::time::sleep(request_delay)),
        )
        .await
        {
            futures::future::Either::Left(_) => {
                trace!("notified!");
                // notified so we send off notification
                to_requester.0.send(requester).ok().unwrap();
                // make this future never resolve
                pending().await
            }
            futures::future::Either::Right(_) => {
                // slept
                requester
                    .request_concurrent_transition::<ServerRequestWins>(request, &mut read_into)
                    .await
            }
        }
    };
    let mut requester_fut = pin!(requester_fut);
    // tiebreak call 1
    let (winner, credit) = match tiebreak(&mut processor_fut, &mut requester_fut).await? {
        // if processor won then extract processor_transition and create a
        // future that resolves to RequesterOrRequesterTransition
        // Then call next::with_processor_transition
        tiebreak::With::Processor(processor_transition) => {
            trace!("processor won");
            need_requester.notify_one();
            let (winner, credit) = processor_transition
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
                .await?;
            (winner, credit)
        }
        // if requester won then extract requester_transition and provide
        // processor_fut (since this thread is the one that handles processing
        // requests)
        // Then call next::with_requester_transition
        tiebreak::With::Requester(requester_transition) => {
            trace!("requester won");
            let (winner, credit) = requester_transition.next(processor_fut).await?;
            (winner, credit)
        }
    };
    match winner {
        crate::cursor::transition::Won::Processor { res, .. } => {
            let res: state::Wrapper<states::winner::client::State> = res;
            let cursor = Cursor::from_cursor_credit(credit, res);
            cursor.wait_to_close().await?;
            SERVER_REACHED.with(|cr| cr.lock().unwrap().replace(Winner::Client));
        }
        crate::cursor::transition::Won::Requester { res } => {
            let res: state::Wrapper<states::winner::server::State> = res;
            let cursor = Cursor::from_cursor_credit(credit, res);
            cursor.wait_to_close().await?;
            SERVER_REACHED.with(|cr| cr.lock().unwrap().replace(Winner::Server));
        }
    }

    Ok(())
}

pub struct ClientHandler;

impl TransitionLeafHandler<entrypoint::ServerRequestWins> for ClientHandler {
    type NextHandler = not_applicable::Handler;

    async fn handle_transition<'a>(
        self,
        duration: method::ReqOf<'a, entrypoint::ServerRequestWins>,
        wrapper_credit: WrapperCredit<entrypoint::ServerRequestWins>,
    ) -> (
        method::ResOf<'a, entrypoint::ServerRequestWins>,
        Self::NextHandler,
    ) {
        tokio::time::sleep(duration).await;
        (
            state::Wrapper::from_wrapper_credit(wrapper_credit),
            not_applicable::Handler,
        )
    }
}

async fn client(
    endpoint: quinn::Endpoint,
    server_addr: SocketAddr,
    (request, request_delay): (Duration, Duration),
) -> anyhow::Result<()> {
    tracing::trace!("running");
    // the machine's clock is paused, so this measures sim-time
    let start = tokio::time::Instant::now();

    let conn = super::connect_to_server(&endpoint, server_addr).await?;

    let e_cursor: Cursor<states::entrypoint::State, Client, _> = Cursor::new(conn);

    let (processor, requester) = e_cursor.into_processor_and_requester(RootHandler(ClientHandler));
    let mut buf = Vec::new();
    let mut processor_fut = pin!(processor.handle_concurrent_transition_request(&mut buf));
    let mut read_into = Vec::new();
    let need_requester = tokio::sync::Notify::new();
    let to_requester = oneshot::channel();
    let requester_fut = async {
        match select(
            pin!(need_requester.notified()),
            pin!(tokio::time::sleep(request_delay)),
        )
        .await
        {
            futures::future::Either::Left(_) => {
                trace!("notified!");
                // notified so we send off requester
                to_requester.0.send(requester).ok().unwrap();
                // make this future never resolve
                pending().await
            }
            futures::future::Either::Right(_) => {
                // slept
                requester
                    .request_concurrent_transition::<ClientRequestWins>(request, &mut read_into)
                    .await
            }
        }
    };
    let mut requester_fut = pin!(requester_fut);
    let (winner, credit) = match tiebreak(&mut processor_fut, &mut requester_fut).await? {
        crate::cursor::transition::tiebreak::With::Processor(processor_transition) => {
            trace!("processor won");
            need_requester.notify_one();
            let (winner, credit) = processor_transition
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
                .await?;
            (winner, credit)
        }
        crate::cursor::transition::tiebreak::With::Requester(requester_transition) => {
            trace!("requester won");
            let (winner, credit) = requester_transition.next(processor_fut).await?;

            (winner, credit)
        }
    };
    match winner {
        crate::cursor::transition::Won::Processor { res, .. } => {
            let res: state::Wrapper<states::winner::server::State> = res;
            let cursor = Cursor::from_cursor_credit(credit, res);
            cursor.close().await?;
            CLIENT_REACHED.with(|cr| cr.lock().unwrap().replace(Winner::Server));
        }
        crate::cursor::transition::Won::Requester { res } => {
            let res: state::Wrapper<states::winner::client::State> = res;
            let cursor = Cursor::from_cursor_credit(credit, res);
            cursor.close().await?;
            CLIENT_REACHED.with(|cr| cr.lock().unwrap().replace(Winner::Client));
        }
    }

    CLIENT_ELAPSED.with(|e| e.lock().unwrap().replace(start.elapsed()));
    Ok(())
}
scoped_thread_local!(static CLIENT_ELAPSED: Mutex<Option<Duration>>);
scoped_thread_local!(static CLIENT_REACHED: Mutex<Option<Winner>>);
scoped_thread_local!(static SERVER_REACHED: Mutex<Option<Winner>>);

/// Runs a hegel test case on a fresh thread.
///
/// The deterministic getrandom backend keeps one rng per thread that is never
/// reseeded, and hegel runs every case (and every replay of a case) on the same
/// thread. A fresh thread makes each run with the same inputs see the same
/// randomness, which hegel needs to replay and shrink failures.
fn on_fresh_thread(tc: TestCase, test: impl FnOnce(TestCase) + Send + 'static) {
    std::thread::spawn(move || test(tc))
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

/// runs a single race, asserts that both sides agree on the winner, and
/// returns the winner along with the client's elapsed sim-time
fn run_race<ClientFut, ServerFut>(
    client: impl Fn(Endpoint, SocketAddr) -> ClientFut + 'static + Copy,
    server: impl Fn(Endpoint) -> ServerFut + 'static + Copy,
    sim_config: impl Into<Option<Config>>,
) -> (Winner, Duration)
where
    ClientFut: Future<Output = anyhow::Result<()>>,
    ServerFut: Future<Output = anyhow::Result<()>>,
{
    let cr = Mutex::default();
    let sr = Mutex::default();
    let elapsed = Mutex::default();
    CLIENT_ELAPSED.set(&elapsed, || {
        CLIENT_REACHED.set(&cr, || {
            SERVER_REACHED.set(&sr, || {
                if let Err(e) = harness(client, server, sim_config) {
                    tracing::error!("harness: {e:?}");
                }
            })
        })
    });
    let client_winner = cr.into_inner().unwrap().expect("client should finish");
    let server_winner = sr.into_inner().unwrap().expect("server should finish");
    assert_eq!(client_winner, server_winner);
    let elapsed = elapsed.into_inner().unwrap().expect("client should finish");
    (client_winner, elapsed)
}

/// makes one of the four durations (client request, client request delay,
/// server request, server request delay) long and returns
/// `(client_durations, server_durations, long_duration)`
fn draw_one_long(tc: &TestCase) -> ((Duration, Duration), (Duration, Duration), Duration) {
    draw_long_among(tc, &[0, 1, 2, 3])
}

/// like [`draw_one_long`] but only ever makes one side's handler long, so both
/// sides always request a transition and race
fn draw_one_long_handler(tc: &TestCase) -> ((Duration, Duration), (Duration, Duration), Duration) {
    draw_long_among(tc, &[0, 2])
}

fn draw_long_among(
    tc: &TestCase,
    indices: &[usize],
) -> ((Duration, Duration), (Duration, Duration), Duration) {
    let which_is_long = tc.draw(gs::sampled_from(indices.to_vec()));
    let mut durations = [Duration::ZERO; 4];
    let long_duration: Duration =
        Duration::from_secs(tc.draw(gs::integers().max_value(20).min_value(1)));
    durations[which_is_long] = long_duration;
    (
        (durations[0], durations[1]),
        (durations[2], durations[3]),
        long_duration,
    )
}

#[test_log::test]
#[hegel::test(test_cases = 8, report_multiple_failures = true)]
fn race_once(tc: TestCase) {
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

/// draws an rng seed from hegel so a config's internal randomness is part of
/// the test case, letting hegel reproduce and shrink failures that depend on
/// it
fn hegel_seeded(tc: &TestCase) -> Config {
    Config {
        rng_seed: tc.draw(gs::integers()),
        ..Default::default()
    }
}

/// A lossless network where every message takes the same time, so requests
/// sent at the same moment cross each other in flight.
fn symmetric_config(tc: &TestCase) -> Config {
    let latency = Duration::from_millis(20);

    Config {
        tick_amount: Duration::from_millis(10),
        latency: Latency {
            min_message_latency: latency,
            max_message_latency: latency,
            ..Default::default()
        },
        message_loss: MessageLoss::ZERO,
        ..hegel_seeded(tc)
    }
}

/// With no delays on a [`symmetric_config`] network both sides request at
/// once and both handlers finish before either reply arrives, so it's a true
/// tie: which side wins depends on the sim's rng, but both sides must still
/// agree (checked by [`run_race`]).
#[test_log::test]
#[hegel::test]
#[ignore = "slow: ~39.5s"]
fn tie(tc: TestCase) {
    run_race(
        |a, b| client(a, b, (Duration::ZERO, Duration::ZERO)),
        |e| server(e, (Duration::ZERO, Duration::ZERO)),
        symmetric_config(&tc),
    );
}

/// a lossy network, seeded by hegel, so that a transition request can arrive
/// after the reply to the peer's own request
fn draw_lossy_config(tc: &TestCase) -> Config {
    let loss_percent: u32 = tc.draw(gs::integers().min_value(0).max_value(5));
    Config {
        tick_amount: Duration::from_millis(10),
        message_loss: MessageLoss::new(f64::from(loss_percent) / 100.0).unwrap(),
        ..hegel_seeded(tc)
    }
}

/// [`race_once`] on a lossy network. When the peer's transition request is
/// lost but the peer's reply to ours is not, the deferring side must keep its
/// processor running until the retransmitted request arrives so it can reject
/// it rather than leak it.
#[test_log::test]
#[hegel::test(test_cases = 32, suppress_health_check = [HealthCheck::TooSlow])]
#[ignore = "slow: ~12.0s"]
fn race_once_lossy(tc: TestCase) {
    on_fresh_thread(tc, |tc| {
        let (client_durations, server_durations, _) = draw_one_long_handler(&tc);
        run_race(
            move |a, b| client(a, b, client_durations),
            move |e| server(e, server_durations),
            draw_lossy_config(&tc),
        );
    });
}

#[hegel::composite]
fn duration_generator(tc: &TestCase) -> Duration {
    Duration::from_millis(tc.draw(gs::integers().min_value(0).max_value(1)) * 1000)
}

#[hegel::composite]
fn durations_generator(tc: &TestCase) -> (Duration, Duration) {
    (tc.draw(duration_generator()), tc.draw(duration_generator()))
}

#[test_log::test]
#[hegel::test(suppress_health_check = [HealthCheck::TooSlow])]
#[ignore = "slow: ~15.7s"]
fn race(tc: TestCase) {
    on_fresh_thread(tc, |tc| {
        let client_durations = tc.draw(durations_generator());
        let server_durations = tc.draw(durations_generator());
        run_race(
            move |a, b| client(a, b, client_durations),
            move |e| server(e, server_durations),
            None,
        );
    });
}

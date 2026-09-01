pub mod states;

use std::{
    future::pending,
    net::SocketAddr,
    pin::pin,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use futures::{FutureExt, future::select};
use hegel::{TestCase, generators as gs};
use quinn::Endpoint;
use scoped_tls::scoped_thread_local;
use tokio::sync::oneshot;
use tracing::trace;

use crate::{
    cursor::{
        Cursor,
        tests::{
            harness,
            race::states::entrypoint::{self, ClientRequestWins, ServerRequestWins},
        },
        transition::{
            RequesterOrRequesterTransition, next_with_processor_transition,
            next_with_requester_transition, tiebreak,
        },
    },
    traits::{
        handler::{TransitionLeafHandler, root_method::RootHandler},
        markers::{Client, Server, not_applicable},
        state,
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
        &mut self,
        duration: crate::traits::method::ReqOf<'a, entrypoint::ClientRequestWins>,
        wrapper_credit: crate::traits::state::WrapperCredit<entrypoint::ClientRequestWins>,
    ) -> (
        crate::traits::method::ResOf<'a, entrypoint::ClientRequestWins>,
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
    let mut processor_fut = processor.handle_transition_request(&mut buf);
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
                    .request_transition::<ServerRequestWins>(request, &mut read_into)
                    .await
            }
        }
    };
    let mut requester_fut = pin!(requester_fut);
    let (winner, credit) = match tiebreak(&mut processor_fut, &mut requester_fut).await {
        crate::cursor::transition::TiebreakResult::Processor(processor_transition) => {
            trace!("processor won");
            let p_transition = processor_transition?;
            need_requester.notify_one();
            let (winner, credit) = next_with_processor_transition(p_transition, async move {
                let res = match select(requester_fut, to_requester.1).await {
                    futures::future::Either::Left((requester_transition, _)) => {
                        RequesterOrRequesterTransition::RequesterTransition(requester_transition?)
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
        crate::cursor::transition::TiebreakResult::Requester(requester_transition) => {
            trace!("requester won");
            let (winner, credit) =
                next_with_requester_transition(requester_transition?, processor_fut).await?;

            (winner, credit)
        }
    };
    match winner {
        crate::cursor::transition::Won::Processor { res, .. } => {
            let res: state::Wrapper<states::winner::client::State> = res;
            let _cursor = Cursor::from_cursor_credit(credit, res);
            SERVER_REACHED.with(|cr| cr.lock().unwrap().replace(Winner::Client));
        }
        crate::cursor::transition::Won::Requester { res } => {
            let res: state::Wrapper<states::winner::server::State> = res;
            let _cursor = Cursor::from_cursor_credit(credit, res);
            SERVER_REACHED.with(|cr| cr.lock().unwrap().replace(Winner::Server));
        }
    }

    Ok(())
}

pub struct ClientHandler;

impl TransitionLeafHandler<entrypoint::ServerRequestWins> for ClientHandler {
    type NextHandler = not_applicable::Handler;

    async fn handle_transition<'a>(
        &mut self,
        duration: crate::traits::method::ReqOf<'a, entrypoint::ServerRequestWins>,
        wrapper_credit: crate::traits::state::WrapperCredit<entrypoint::ServerRequestWins>,
    ) -> (
        crate::traits::method::ResOf<'a, entrypoint::ServerRequestWins>,
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

    let conn = super::connect_to_server(&endpoint, server_addr).await?;

    let e_cursor: Cursor<states::entrypoint::State, Client, _> = Cursor::new(conn);

    let (processor, requester) = e_cursor.into_processor_and_requester(RootHandler(ClientHandler));
    let mut buf = Vec::new();
    let mut processor_fut = processor.handle_transition_request(&mut buf);
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
                    .request_transition::<ClientRequestWins>(request, &mut read_into)
                    .await
            }
        }
    };
    let mut requester_fut = pin!(requester_fut);
    let (winner, credit) = match tiebreak(&mut processor_fut, &mut requester_fut).await {
        crate::cursor::transition::TiebreakResult::Processor(processor_transition) => {
            trace!("processor won");
            let p_transition = processor_transition?;
            need_requester.notify_one();
            let (winner, credit) = next_with_processor_transition(p_transition, async move {
                let res = match select(requester_fut, to_requester.1).await {
                    futures::future::Either::Left((requester_transition, _)) => {
                        RequesterOrRequesterTransition::RequesterTransition(requester_transition?)
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
        crate::cursor::transition::TiebreakResult::Requester(requester_transition) => {
            trace!("requester won");
            let (winner, credit) =
                next_with_requester_transition(requester_transition?, processor_fut).await?;

            (winner, credit)
        }
    };
    match winner {
        crate::cursor::transition::Won::Processor { res, .. } => {
            let res: state::Wrapper<states::winner::server::State> = res;
            let _cursor = Cursor::from_cursor_credit(credit, res);
            CLIENT_REACHED.with(|cr| cr.lock().unwrap().replace(Winner::Server));
        }
        crate::cursor::transition::Won::Requester { res } => {
            let res: state::Wrapper<states::winner::client::State> = res;
            let _cursor = Cursor::from_cursor_credit(credit, res);
            CLIENT_REACHED.with(|cr| cr.lock().unwrap().replace(Winner::Client));
        }
    }

    Ok(())
}
scoped_thread_local!(static CLIENT_REACHED: Mutex<Option<Winner>>);
scoped_thread_local!(static SERVER_REACHED: Mutex<Option<Winner>>);

#[hegel::composite]
fn duration_generator(tc: &TestCase) -> Duration {
    Duration::from_millis(tc.draw(gs::integers().min_value(0).max_value(10)) * 10)
}

#[hegel::composite]
fn durations_generator(tc: &TestCase) -> (Duration, Duration) {
    (tc.draw(duration_generator()), tc.draw(duration_generator()))
}

#[test_log::test]
fn race_once() {
    let client_durations = (Duration::from_millis(100000), Duration::from_millis(0));
    let server_durations = (Duration::from_millis(0), Duration::from_millis(0));
    let cr = Default::default();
    let sr = Default::default();
    CLIENT_REACHED.set(&cr, || {
        SERVER_REACHED
            .set(&sr, || {
                harness(
                    move |a, b| client(a, b, client_durations),
                    move |e| server(e, server_durations),
                )
                .ok();
                CLIENT_REACHED.with(|cr| {
                    SERVER_REACHED.with(|sr| {
                        assert_eq!(
                            cr.lock().unwrap().as_ref().unwrap(),
                            sr.lock().unwrap().as_ref().unwrap()
                        )
                    })
                });
                anyhow::Ok(())
            })
            .unwrap();
    })
}

#[hegel::test(test_cases = 100)]
#[ignore]
fn race(tc: TestCase) {
    let client_durations = tc.draw(durations_generator());
    let server_durations = tc.draw(durations_generator());
    let cr = Default::default();
    let sr = Default::default();
    CLIENT_REACHED.set(&cr, || {
        SERVER_REACHED
            .set(&sr, || {
                harness(
                    move |a, b| client(a, b, client_durations),
                    move |e| server(e, server_durations),
                )
                .ok();
                CLIENT_REACHED.with(|cr| {
                    SERVER_REACHED.with(|sr| {
                        assert_eq!(
                            cr.lock().unwrap().as_ref().unwrap(),
                            sr.lock().unwrap().as_ref().unwrap()
                        )
                    })
                });
                anyhow::Ok(())
            })
            .unwrap();
    })
}

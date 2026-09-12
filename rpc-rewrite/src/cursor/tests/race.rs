pub mod states;

use std::{future::pending, net::SocketAddr, pin::pin, sync::Mutex, time::Duration};

use futures::future::select;
use hegel::{TestCase, generators as gs};
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

#[hegel::composite]
fn duration_generator(tc: &TestCase) -> Duration {
    Duration::from_millis(tc.draw(gs::integers().min_value(0).max_value(1)) * 1000)
}

#[hegel::composite]
fn durations_generator(tc: &TestCase) -> (Duration, Duration) {
    (tc.draw(duration_generator()), tc.draw(duration_generator()))
}

#[hegel::test]
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

use std::{
    convert::Infallible,
    mem,
    pin::pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::{FutureExt, future::join, select, select_biased};
use hegel::TestCase;
use tracing::{Instrument, debug, info_span};

use crate::{
    in_memory_transport,
    machine_cursor::{
        MachineCursorClient, MachineCursorServer, Requester,
        transition::{RequestTransition, requester::RequesterTransition, tiebreak},
    },
};

use super::final_endpoint::FinalEndpoint;

pub use super::prelude::*;

async fn server(
    request: Option<impl Future<Output = Request> + Send + 'static>,
    conn: in_memory_transport::Connection<u8>,
) -> FinalEndpoint<crate::state::role::Server> {
    let cursor = MachineCursorServer::<Entrypoint, _>::new(conn);
    let (processor, requester) = cursor.into_parts(server::Method);
    let (to_sacrifice, to_processor_transition) = processor.handle_transition_request();

    enum RequesterState {
        Requester(
            Requester<
                Entrypoint,
                crate::state::role::Server,
                client::Method,
                in_memory_transport::Connection<u8>,
            >,
        ),
        Transition(
            RequesterTransition<
                Entrypoint,
                RequestTransition<
                    Request,
                    client::Method,
                    crate::state::role::Server,
                    in_memory_transport::Connection<u8>,
                >,
            >,
        ),
        Taken,
    }

    impl RequesterState {
        pub fn request_transition(&mut self, request: Request) {
            let curr = mem::replace(self, Self::Taken);
            match curr {
                RequesterState::Requester(requester) => {
                    *self = Self::Transition(requester.request_transition(request));
                }
                _ => return,
            }
        }

        pub fn take(&mut self) -> RequesterState {
            mem::replace(self, Self::Taken)
        }
    }

    let out: FinalEndpoint<_> = if let Some(request) = request {
        let requester = Arc::new(Mutex::new(RequesterState::Requester(requester)));
        let request_jh = {
            let requester = requester.clone();
            tokio::spawn(async move {
                let request = request.await;
                let mut requester_lock = requester.lock().unwrap();

                requester_lock.request_transition(request);
            })
        };
        let abort_handle = request_jh.abort_handle();
        let request = pin!(request_jh);
        let mut request_fut = request.fuse();
        let to_processor_transition = pin!(to_processor_transition);
        let mut to_processor_transition = to_processor_transition.fuse();

        let processor_transition = select! {
            processor_transition = to_processor_transition => {
                Some(processor_transition.unwrap())
            }
            _request = request_fut => {
                None
            },
        };

        abort_handle.abort();

        match requester.lock().unwrap().take() {
            RequesterState::Requester(requester) => {
                let transition = match processor_transition {
                    Some(pt) => pt,
                    None => to_processor_transition.await.unwrap(),
                };
                let transition = transition.next_with_requester(requester).await.unwrap();
                let (res, transition) = transition.extract_res();
                transition.finish(res).into()
            }
            RequesterState::Transition(requester_transition) => {
                debug!("transition got through; running tiebreak");
                let tiebreak_result = match processor_transition {
                    Some(processor_transition) => {
                        tiebreak::tiebreak::<_, _, _, _, _, _, Infallible>(
                            processor_transition,
                            requester_transition,
                            to_sacrifice,
                        )
                        .await
                        .unwrap()
                    }
                    None => tiebreak::from_processor_to_completion(
                        to_processor_transition,
                        to_sacrifice,
                        requester_transition,
                    )
                    .await
                    .unwrap(),
                };
                match tiebreak_result {
                    tiebreak::TiebreakResult::ProcessorWon(finalize_processor_transition) => {
                        let (res, transition) = finalize_processor_transition.extract_res();
                        transition.finish(res).await.unwrap().into()
                    }
                    tiebreak::TiebreakResult::RequesterWon(finalize_requester_transition) => {
                        let (res, transition) = finalize_requester_transition.extract_res();
                        transition.finish(res).await.unwrap().into()
                    }
                }
            }
            RequesterState::Taken => {
                unreachable!()
            }
        }
    } else {
        let processor_transition = to_processor_transition.await.unwrap();
        let processor_transition = processor_transition
            .next_with_requester(requester)
            .await
            .unwrap();
        let (res, processor_transition) = processor_transition.extract_res();
        processor_transition.finish(res).into()
    };
    out
}

async fn client(
    request: Option<impl Future<Output = Request> + Send + 'static>,
    conn: in_memory_transport::Connection<u8>,
) -> FinalEndpoint<crate::state::role::Client> {
    let cursor = MachineCursorClient::<Entrypoint, _>::new(conn);
    let (processor, requester) = cursor.into_parts(client::Method);
    let (to_sacrifice, to_processor_transition) = processor.handle_transition_request();

    enum RequesterState {
        Requester(
            Requester<
                Entrypoint,
                crate::state::role::Client,
                server::Method,
                in_memory_transport::Connection<u8>,
            >,
        ),
        Transition(
            RequesterTransition<
                Entrypoint,
                RequestTransition<
                    Request,
                    server::Method,
                    crate::state::role::Client,
                    in_memory_transport::Connection<u8>,
                >,
            >,
        ),
        Taken,
    }

    impl RequesterState {
        pub fn request_transition(&mut self, request: Request) {
            let curr = mem::replace(self, Self::Taken);
            match curr {
                RequesterState::Requester(requester) => {
                    *self = Self::Transition(requester.request_transition(request));
                }
                _ => return,
            }
        }

        pub fn take(&mut self) -> RequesterState {
            mem::replace(self, Self::Taken)
        }
    }

    let out: FinalEndpoint<_> = if let Some(request) = request {
        let requester = Arc::new(Mutex::new(RequesterState::Requester(requester)));
        let request_jh = {
            let requester = requester.clone();
            tokio::spawn(async move {
                let request = request.await;
                let mut requester_lock = requester.lock().unwrap();

                requester_lock.request_transition(request);
            })
        };
        let abort_handle = request_jh.abort_handle();
        let request = pin!(request_jh);
        let mut request_fut = request.fuse();
        let to_processor_transition = pin!(to_processor_transition);
        let mut to_processor_transition = to_processor_transition.fuse();

        let processor_transition = select! {
            processor_transition = to_processor_transition => {
                Some(processor_transition.unwrap())
            }
            _request = request_fut => {
                None
            },
        };

        abort_handle.abort();

        match requester.lock().unwrap().take() {
            RequesterState::Requester(requester) => {
                let transition = match processor_transition {
                    Some(pt) => pt,
                    None => to_processor_transition.await.unwrap(),
                };
                let transition = transition.next_with_requester(requester).await.unwrap();
                let (res, transition) = transition.extract_res();
                transition.finish(res).into()
            }
            RequesterState::Transition(requester_transition) => {
                debug!("transition got through; running tiebreak");
                let tiebreak_result = match processor_transition {
                    Some(processor_transition) => {
                        tiebreak::tiebreak::<_, _, _, _, _, _, Infallible>(
                            processor_transition,
                            requester_transition,
                            to_sacrifice,
                        )
                        .await
                        .unwrap()
                    }
                    None => tiebreak::from_processor_to_completion(
                        to_processor_transition,
                        to_sacrifice,
                        requester_transition,
                    )
                    .await
                    .unwrap(),
                };
                match tiebreak_result {
                    tiebreak::TiebreakResult::ProcessorWon(finalize_processor_transition) => {
                        let (res, transition) = finalize_processor_transition.extract_res();
                        transition.finish(res).await.unwrap().into()
                    }
                    tiebreak::TiebreakResult::RequesterWon(finalize_requester_transition) => {
                        let (res, transition) = finalize_requester_transition.extract_res();
                        transition.finish(res).await.unwrap().into()
                    }
                }
            }
            RequesterState::Taken => {
                unreachable!()
            }
        }
    } else {
        let processor_transition = to_processor_transition.await.unwrap();
        let processor_transition = processor_transition
            .next_with_requester(requester)
            .await
            .unwrap();
        let (res, processor_transition) = processor_transition.extract_res();
        processor_transition.finish(res).into()
    };
    out
}

use hegel::generators as gs;

#[hegel::composite]
fn generate_request(tc: TestCase) -> Option<Request> {
    if tc.draw(gs::booleans()) {
        None
    } else {
        let ms = tc
            .draw(gs::optional(gs::integers().max_value(10000)))
            .map(|v| Duration::from_millis(v));
        Some(Request {
            priority: tc.draw(gs::booleans()),
            sleep: ms,
        })
    }
}

#[hegel::test(test_cases = 100_000)]
fn fuzz_tiebreak(tc: TestCase) {
    tokio::runtime::Builder::new_current_thread()
        .start_paused(true)
        .enable_all()
        .rng_seed(tokio::runtime::RngSeed::from_bytes(&tc.draw(gs::arrays::<
            _,
            _,
            1,
        >(
            gs::integers(),
        ))))
        .build()
        .unwrap()
        .block_on(async move {
            let request_server: Option<Request> = tc.draw(generate_request());
            let request_client: Option<Request> = tc.draw(generate_request());
            // one of them must transition; otherwise would hang
            tc.assume(request_client.is_some() || request_server.is_some());

            let network = in_memory_transport::Network::new();
            let ConnPair {
                server_conn,
                client_conn,
            } = setup_conn(0, 1, &network).await;
            let before_transitioning_server = tc.draw(gs::integers().max_value(1000));
            let server = server(
                request_server.map(|request| async move {
                    tokio::time::sleep(Duration::from_millis(before_transitioning_server)).await;
                    request
                }),
                server_conn,
            )
            .instrument(info_span!("server"));

            let before_transitioning_client = tc.draw(gs::integers().max_value(1000));

            let client = client(
                request_client.map(|request| async move {
                    tokio::time::sleep(Duration::from_millis(before_transitioning_client)).await;
                    request
                }),
                client_conn,
            )
            .instrument(info_span!("client"));

            let (client, server) = join(client, server).await;

            assert_eq!(client, server);
        });
}

pub mod client_conn;
pub mod server_conn;

#[cfg(test)]
mod tests {
    use crate::state_machine_transitions::tests::actions::{
        LogoutMethod, LogoutRequest, LongPingMethod, LongPingRequest, PingMethod, PingRequest,
    };
    use crate::state_machine_transitions::tests::authenticate::LoginMethod;
    use crate::transport::Incoming as _;
    use futures::TryStreamExt as _;
    use futures::stream::FuturesUnordered;
    use tokio::task::JoinSet;
    use tokio::time::Instant;
    use tracing::debug;

    use crate::state_machine_transitions::tests::{actions, authenticate};
    use crate::wrappers::server_conn;
    use crate::{MemoryTransport, Transport, in_memory_transport};

    #[tokio::test]
    async fn wrapper() {
        let network = in_memory_transport::Network::new();

        let mut js = JoinSet::new();
        // server
        let net1 = network.clone();
        let tp = MemoryTransport::new(net1, 1u64);
        let server_addr = tp.address();
        js.spawn(async move {
            use server_conn::Wrapper;
            let handler = authenticate::LoginMethod;
            let incoming = tp.accept().await.unwrap();
            let conn = incoming.accept().await.unwrap();
            let mut login = Wrapper::new(handler, conn);
            'outer: loop {
                let mut logged_in = loop {
                    let Ok(parts) = login.handle_state_transition().await else {
                        debug!("breaking out of server loop due to login handling error");
                        break 'outer;
                    };

                    let (res, parts) = parts.extract_res();

                    match res {
                        authenticate::Responses::Ok(method_wrapper) => {
                            break Wrapper::from_parts(
                                parts.method_change(method_wrapper, actions::RootHandler),
                            );
                        }
                        authenticate::Responses::TryAgain(method_wrapper) => {
                            login = Wrapper::from_parts(parts.method_restore(method_wrapper))
                        }
                    }
                };

                login = loop {
                    let Ok(parts) = logged_in
                        .handle_loopback_partial(actions::LoopbackHandler)
                        .await
                    else {
                        return;
                    };
                    let (res, parts) = parts.extract_res();
                    match res {
                        actions::NextHandler::Actions(method_wrapper) => {
                            logged_in = parts.method_restore(method_wrapper).into();
                        }
                        actions::NextHandler::Authenticate(method_wrapper) => {
                            break parts
                                .method_change(method_wrapper, authenticate::LoginMethod)
                                .into();
                        }
                    }
                };
            }
        });
        // client
        let net2 = network.clone();
        js.spawn(async move {
            use crate::wrappers::client_conn::Wrapper;

            let tp = MemoryTransport::new(net2, 2u64);
            let conn = tp.connect(&server_addr).await.unwrap();
            let mut login = Wrapper::<LoginMethod, _>::new(conn);

            let (res, parts) = login
                .query::<LoginMethod>(authenticate::LoginRequest::new(true))
                .await
                .unwrap()
                .extract_res();

            login = match res {
                authenticate::Responses::Ok(_method_wrapper) => panic!("set try_again to true"),
                authenticate::Responses::TryAgain(method_wrapper) => {
                    parts.set_method(method_wrapper).into()
                }
            };

            let mut actions: Wrapper<_, _> = login
                .query::<LoginMethod>(authenticate::LoginRequest::new(false))
                .await
                .unwrap()
                .map_res(|v| match v {
                    authenticate::Responses::Ok(method) => method,
                    _ => panic!(),
                })
                .into();

            let set = FuturesUnordered::new();

            let now = Instant::now();

            set.push(
                actions.query_loopback_child::<LongPingMethod, actions::LoopbackHandler>(
                    LongPingRequest {
                        duration_millis: 100,
                    },
                ),
            );
            set.push(
                actions.query_loopback_child::<LongPingMethod, actions::LoopbackHandler>(
                    LongPingRequest {
                        duration_millis: 100,
                    },
                ),
            );

            let diff = now.elapsed();

            let _results = set.try_collect::<Vec<_>>().await.unwrap();

            assert!(
                diff.as_millis() < 150,
                "should take less time than it would if requests were processed serially"
            );

            actions = actions
                .query::<PingMethod>(PingRequest())
                .await
                .unwrap()
                .map_res(|v| v.0)
                .into();

            login = actions
                .query::<LogoutMethod>(LogoutRequest())
                .await
                .unwrap()
                .map_res(|v| v.0)
                .into();

            drop(login)
        });

        js.join_all().await;
    }
}

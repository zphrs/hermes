mod earth_root;
pub mod entrypoint;
pub mod find_nodes;
pub mod sky_root;

#[cfg(test)]
mod tests {
    use rpc::{
        MemoryTransport, MethodWrapper, Transport, in_memory_transport,
        transport::{Client, Incoming},
    };
    use std::net::IpAddr;
    use tokio::task::JoinSet;
    use tracing::info;

    use crate::{
        api::{
            entrypoint::{self, as_sky},
            find_nodes::KadRpcManager,
            sky_root,
        },
        quinn_transport,
    };

    #[tokio::test]
    #[test_log::test]
    async fn sky_login() {
        let network = in_memory_transport::Network::new();
        let mut js = JoinSet::new();

        let net1 = network.clone();
        // server
        let tp = MemoryTransport::new(net1, IpAddr::from([196, 168, 0, 1]));
        let server_addr = tp.address();
        js.spawn(async move {
            info!("server entered");

            let incoming = tp.accept().await.unwrap();
            let conn = incoming.accept().await.unwrap();
            let wrapper = MethodWrapper::<entrypoint::Method>::default();
            let mut handler = entrypoint::Method::new(IpAddr::from(*conn.remote_addr()));
            let stream = conn.accept_stream().await.unwrap();
            let res = wrapper
                .handle_state_transition_request(&conn, stream, &mut handler)
                .await
                .unwrap();

            let (actions, sky_node) = {
                use as_sky::Response::*;
                match res {
                    entrypoint::Response::Sky(Ok(method_wrapper), sky_node) => {
                        (method_wrapper, sky_node)
                    }
                    entrypoint::Response::Sky(Invalid(_method_wrapper), _) => unimplemented!(),
                }
            };
            let rpc_manager = KadRpcManager::new(
                quinn_transport::Transport::self_signed_server()
                    .await
                    .unwrap(),
                server_addr.into(),
            );
            let concurrent_handler = sky_root::Method::new(&rpc_manager, sky_node);
            actions
                .handle_only_concurrent_requests(&conn, concurrent_handler)
                .await
                .expect_err("should fail with a broken pipe");
            info!("server exited")
        });
        // client
        let net2 = network.clone();
        let tp = MemoryTransport::new(net2, IpAddr::from([196, 168, 0, 2]));
        js.spawn(async move {
            let conn = tp.connect(&server_addr).await.unwrap();
            let wrapper = MethodWrapper::<entrypoint::Method>::default();
            let res = wrapper
                .query::<as_sky::Method, _>(as_sky::Request::new(), &conn)
                .await
                .unwrap();
            let actions = {
                use as_sky::Response;
                match res {
                    Response::Ok(method_wrapper) => method_wrapper,
                    Response::Invalid(_method_wrapper) => unimplemented!(),
                }
            };
            let _res = actions
                .query_loopback::<sky_root::Method, _>(
                    sky_root::Request::Ping(shared_schema::ping::Request),
                    &conn,
                )
                .await
                .unwrap();
        });
        js.join_all().await;
    }
}

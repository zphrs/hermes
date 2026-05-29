pub mod entrypoint;
mod sky_root;

#[cfg(test)]
mod tests {
    use rpc::{
        MemoryTransport, MethodWrapper, Transport, in_memory_transport,
        transport::{Client, Incoming},
    };
    use std::net::IpAddr;
    use tokio::task::JoinSet;
    use tracing::info;

    use crate::api::{
        entrypoint::{self, as_sky},
        sky_root,
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
            let maybe_logged_in = match res {
                entrypoint::Response::Sky(response) => response,
            };
            let actions = {
                use as_sky::Response::*;
                match maybe_logged_in {
                    Ok(method_wrapper) => method_wrapper,
                    Invalid(_method_wrapper) => unimplemented!(),
                }
            };
            let concurrent_handler = sky_root::Method;
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
                .concurrent_root_query::<sky_root::Method, _>(
                    sky_root::Request::Ping(shared_schema::ping::Request),
                    &conn,
                )
                .await
                .unwrap();
        });
        js.join_all().await;
    }
}

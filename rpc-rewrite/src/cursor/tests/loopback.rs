use std::net::SocketAddr;

use super::super::Cursor;
use super::{accept_client, connect_to_server};
use crate::traits::{
    handler::root_method::RootHandler,
    markers::{self, not_applicable},
};

mod ping {
    use minicbor::bytes::ByteSlice;

    use crate::traits::{
        self, LeafHandler,
        handler::root_method::RootMethod,
        markers::{False, NotApplicable},
    };

    pub struct Method;

    impl traits::Method for Method {
        type Req<'buf> = &'buf ByteSlice;

        type Res<'buf> = &'buf ByteSlice;

        type Transitions = False;

        type HasDescendants = False;
    }

    impl LeafHandler for Method {
        async fn handle<'a>(
            &mut self,
            request: traits::method::ReqOf<'a, Self>,
        ) -> traits::method::ResOf<'a, Self> {
            request
        }
    }

    pub struct State;

    impl traits::state::Entrypoint for State {}

    impl traits::State for State {
        type ClientHandles = NotApplicable;

        type ServerHandles = RootMethod<Method>;
    }
}

async fn server(endpoint: quinn::Endpoint) -> anyhow::Result<()> {
    let connection = accept_client(&endpoint).await?;
    let cursor = Cursor::<ping::State, markers::Server, _>::new(connection);
    let (processor, _requester) = cursor.into_processor_and_requester(RootHandler(ping::Method));

    // we expect an error out here
    let _err = processor.handle_loopback_requests().await;

    Ok(())
}

async fn client(endpoint: quinn::Endpoint, server_addr: SocketAddr) -> anyhow::Result<()> {
    let connection = connect_to_server(&endpoint, server_addr).await?;
    let cursor = Cursor::<ping::State, markers::Client, _>::new(connection);

    let (_processor, requester) = cursor.into_processor_and_requester(not_applicable::Handler);
    let mut read_buf = Vec::new();
    requester
        .request_loopback::<ping::Method>(b"Hello, World!".as_ref().into(), &mut read_buf)
        .await?;
    Ok(())
}

#[test_log::test]
fn loopback() -> anyhow::Result<()> {
    super::harness(client, server)
}

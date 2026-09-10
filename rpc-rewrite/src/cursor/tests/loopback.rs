use std::net::SocketAddr;

use super::super::Cursor;
use super::{accept_client, connect_to_server};
use crate::markers::{self, not_applicable};
use crate::method::handler::root_method::RootHandler;

mod ping {
    use minicbor::bytes::ByteSlice;

    use crate::{
        markers::{False, NotApplicable},
        method::{self, LeafHandler, handler::root_method::RootMethod},
    };

    pub struct Method;

    impl method::Method for Method {
        type Req<'buf> = &'buf ByteSlice;

        type Res<'buf> = &'buf ByteSlice;

        type Transitions = False;

        type HasDescendants = False;
    }

    impl LeafHandler for Method {
        async fn handle<'a>(
            &mut self,
            request: method::ReqOf<'a, Self>,
        ) -> method::ResOf<'a, Self> {
            request
        }
    }

    pub struct State;

    impl crate::cursor::state::Entrypoint for State {}

    impl crate::cursor::State for State {
        type ClientHandles = NotApplicable;

        type ServerHandles = RootMethod<Method>;
    }
}

async fn server(endpoint: quinn::Endpoint) -> anyhow::Result<()> {
    let connection = accept_client(&endpoint).await?;
    let cursor = Cursor::<ping::State, markers::Server, _>::new(connection);
    let (processor, _requester) = cursor.into_processor_and_requester(RootHandler(ping::Method));

    // we expect an error out here once connection drops
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

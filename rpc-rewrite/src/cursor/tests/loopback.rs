use std::net::SocketAddr;

use super::super::Cursor;
use super::{accept_client, connect_to_server};
use crate::Method;
use crate::marker::{self, Leaf, Loopback, not_applicable};
use crate::method::handler::root_method::RootHandler;

pub enum RootRequest {
    A(ARequest),
    B(BRequest),
}

pub struct ARequest;

pub struct AMethod;

pub struct BMethod;

impl Method for AMethod {
    type Req<'buf> = ARequest;

    type Res<'buf> = ();

    type Type = Leaf<Loopback>;
}

pub struct BRequest;

mod ping {
    use minicbor::bytes::ByteSlice;

    use crate::{
        marker::{Loopback, NotApplicable},
        method::{self, LeafHandler, handler::root_method::RootMethod},
    };

    #[derive(Clone)]
    pub struct Method;

    impl method::Method for Method {
        type Req<'buf> = &'buf ByteSlice;

        type Res<'buf> = &'buf ByteSlice;

        type Type = method::LeafLoopback;
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
        type ClientBranchType = Loopback;
        type ClientHandles = NotApplicable;

        type ServerBranchType = Loopback;
        type ServerHandles = RootMethod<Method, Loopback>;
    }
}

async fn server(endpoint: quinn::Endpoint) -> anyhow::Result<()> {
    let connection = accept_client(&endpoint).await?;
    let cursor = Cursor::<ping::State, marker::Server, _>::new(connection);
    let (processor, _requester) = cursor.into_processor_and_requester(RootHandler(ping::Method));

    // we expect an error out here once connection drops
    let _err = processor.handle_loopback_requests().await;

    Ok(())
}

async fn client(endpoint: quinn::Endpoint, server_addr: SocketAddr) -> anyhow::Result<()> {
    let connection = connect_to_server(&endpoint, server_addr).await?;
    let cursor = Cursor::<ping::State, marker::Client, _>::new(connection);

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

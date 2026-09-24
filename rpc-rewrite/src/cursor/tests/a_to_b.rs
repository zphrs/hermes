use std::{net::SocketAddr, sync::Mutex, time::Duration};

use scoped_tls::scoped_thread_local;

use crate::{
    cursor::Cursor,
    cursor::{requester::transition::RequestTransitionError, state},
    io,
    marker::{self, not_applicable},
    method::handler::root_method::RootHandler,
};

mod a;
mod b;
impl state::Entrypoint for a::State {}
async fn server(endpoint: quinn::Endpoint) -> anyhow::Result<()> {
    let connection = super::accept_client(&endpoint).await?;
    let a_cursor = Cursor::<a::State, marker::Server, _>::new(connection);
    let (processor, requester) = a_cursor.into_processor_and_requester(RootHandler(a::Method));
    let mut buf = Vec::new();
    // we expect an error out here
    let (res, _next_handler, cursor_credit) = processor
        .handle_transition_request(&mut buf, requester)
        .await?;

    let b_cursor = Cursor::from_cursor_credit(cursor_credit, res);

    b_cursor.wait_to_close().await?;

    Ok(())
}

async fn client(endpoint: quinn::Endpoint, server_addr: SocketAddr) -> anyhow::Result<()> {
    let connection = super::connect_to_server(&endpoint, server_addr).await?;
    let cursor = Cursor::<a::State, marker::Client, _>::new(connection);

    let (processor, requester) = cursor.into_processor_and_requester(not_applicable::Handler);
    let mut read_buf = Vec::new();
    let (res, cursor_credit) = requester
        .request_transition::<a::Method>((), &mut read_buf, processor)
        .await?;

    let b_cursor = Cursor::from_cursor_credit(cursor_credit, res);
    b_cursor.close().await?;

    Ok(())
}

#[test_log::test]
fn transition() -> anyhow::Result<()> {
    super::harness(client, server, None)
}

/// Handles the transition request up to holding its reply and then drops the
/// held reply, as if the processor was dropped mid-transition.
async fn abandoning_server(endpoint: quinn::Endpoint) -> anyhow::Result<()> {
    let connection = super::accept_client(&endpoint).await?;
    let a_cursor = Cursor::<a::State, marker::Server, _>::new(connection);
    let (processor, _requester) = a_cursor.into_processor_and_requester(RootHandler(a::Method));
    let mut buf = Vec::new();
    let processor_transition = processor
        .handle_concurrent_transition_request(&mut buf)
        .await?;
    drop(processor_transition);
    // bounded (sim-time) so the sim can go idle once the client is done;
    // keeps the connection open so the client sees the stream end rather
    // than the connection close
    tokio::time::sleep(Duration::from_secs(5)).await;
    Ok(())
}

async fn abandoned_client(
    endpoint: quinn::Endpoint,
    server_addr: SocketAddr,
) -> anyhow::Result<()> {
    let connection = super::connect_to_server(&endpoint, server_addr).await?;
    let cursor = Cursor::<a::State, marker::Client, _>::new(connection);

    let (processor, requester) = cursor.into_processor_and_requester(not_applicable::Handler);
    let mut read_buf = Vec::new();
    let res = requester
        .request_transition::<a::Method>((), &mut read_buf, processor)
        .await;
    let empty = matches!(
        res,
        Err(RequestTransitionError::Read(io::read::Error::Empty))
    );
    EMPTY.with(|e| *e.lock().unwrap() = Some(empty));
    Ok(())
}

scoped_thread_local!(static EMPTY: Mutex<Option<bool>>);

/// A peer that drops its processor without replying is distinct from one that
/// explicitly rejects the transition.
#[test_log::test]
fn abandoned_transition_is_not_rejection() {
    let empty = Mutex::new(None);
    EMPTY.set(&empty, || {
        super::harness(abandoned_client, abandoning_server, None).ok();
    });
    assert_eq!(*empty.lock().unwrap(), Some(true));
}

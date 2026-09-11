use std::net::SocketAddr;

use crate::{
    cursor::Cursor,
    cursor::state,
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

    let _b_cursor = Cursor::from_cursor_credit(cursor_credit, res);

    Ok(())
}

#[test_log::test]
fn transition() -> anyhow::Result<()> {
    super::harness(client, server)
}

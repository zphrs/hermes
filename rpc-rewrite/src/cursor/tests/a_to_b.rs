use std::{convert::Infallible, future::ready, net::SocketAddr};

use anyhow::{Context, anyhow};

use crate::{
    cursor::{
        Cursor,
        transition::{
            RequesterOrRequesterTransition, Won, next_with_processor_transition,
            next_with_requester_transition,
        },
    },
    traits::{
        handler::root_method::RootHandler,
        markers::{self, not_applicable},
        state,
    },
};

mod a;
mod b;
impl state::Entrypoint for a::State {}
async fn server(endpoint: quinn::Endpoint) -> anyhow::Result<()> {
    let connection = super::accept_client(&endpoint).await?;
    let a_cursor = Cursor::<a::State, markers::Server, _>::new(connection);
    let (processor, requester) = a_cursor.into_processor_and_requester(RootHandler(a::Method));
    let mut buf = Vec::new();
    // we expect an error out here
    let processor_transition = processor.handle_transition_request(&mut buf).await?;
    let requester = RequesterOrRequesterTransition::from(requester);
    let (won, cursor_credit) = next_with_processor_transition(
        processor_transition,
        ready(Result::<_, Infallible>::Ok(requester)),
    )
    .await
    .with_context(|| anyhow!("server"))?;
    let (res, _b_handler) = match won {
        Won::Processor { res, next_handler } => (res, next_handler),
        Won::Requester { res } => match res {},
    };
    let b_cursor = Cursor::from_cursor_credit(cursor_credit, res);

    b_cursor.wait_to_close().await?;

    Ok(())
}

async fn client(endpoint: quinn::Endpoint, server_addr: SocketAddr) -> anyhow::Result<()> {
    let connection = super::connect_to_server(&endpoint, server_addr).await?;
    let cursor = Cursor::<a::State, markers::Client, _>::new(connection);

    let (processor, requester) = cursor.into_processor_and_requester(not_applicable::Handler);
    let mut read_buf = Vec::new();
    let requester_transition = requester
        .request_transition::<a::Method>((), &mut read_buf)
        .await?;
    let (won, cursor_credit) =
        next_with_requester_transition(requester_transition, processor.into())
            .await
            .with_context(|| anyhow!("client"))?;

    let res = match won {
        Won::Processor { res, .. } => match res {},
        Won::Requester { res } => res,
    };

    let _b_cursor = Cursor::from_cursor_credit(cursor_credit, res);

    Ok(())
}

#[test_log::test]
fn transition() -> anyhow::Result<()> {
    super::harness(client, server)
}

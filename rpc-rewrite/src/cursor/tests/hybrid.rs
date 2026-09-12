pub mod states;
use std::{convert::Infallible, net::SocketAddr};

use crate::{
    cursor::{Cursor, transition::next},
    marker::{self, not_applicable},
};

use states::entrypoint;

async fn server(endpoint: quinn::Endpoint) -> anyhow::Result<()> {
    let connection = super::accept_client(&endpoint).await?;
    let a_cursor = Cursor::<entrypoint::State, marker::Server, _>::new(connection);
    let (processor, requester) = a_cursor.into_processor_and_requester(entrypoint::RootMethod);
    let mut buf = Vec::new();
    // we expect an error out here
    let p_transition = processor
        .handle_hybrid_concurrent_transition_requests(&mut buf)
        .await?;

    let (won, credit) = next::with_processor_transition(p_transition, async {
        Result::<_, Infallible>::Ok(requester.into())
    })
    .await?;

    let (res, _next_handler) = match won {
        crate::cursor::transition::Won::Processor { res, next_handler } => (res, next_handler),
        crate::cursor::transition::Won::Requester { res } => match res {},
    };

    let wrapper = match res {
        entrypoint::RootResponse::Transition(wrapper) => wrapper,
        _ => unimplemented!(),
    };

    let b_cursor = Cursor::from_cursor_credit(credit, wrapper);

    b_cursor.wait_to_close().await?;

    Ok(())
}

async fn client(endpoint: quinn::Endpoint, server_addr: SocketAddr) -> anyhow::Result<()> {
    let connection = super::connect_to_server(&endpoint, server_addr).await?;
    let cursor = Cursor::<entrypoint::State, marker::Client, _>::new(connection);

    let (processor, requester) = cursor.into_processor_and_requester(not_applicable::Handler);
    let mut read_buf = Vec::new();
    let (res, cursor_credit) = requester
        .request_transition::<entrypoint::Transition>((), &mut read_buf, processor)
        .await?;

    let b_cursor = Cursor::from_cursor_credit(cursor_credit, res);
    b_cursor.close().await;

    Ok(())
}

#[test_log::test]
fn immediate_transition() -> anyhow::Result<()> {
    super::harness(client, server)
}

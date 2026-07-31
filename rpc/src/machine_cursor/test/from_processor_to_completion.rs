use std::time::Duration;

use futures::future::join;
use hegel::TestCase;
use tracing::{Instrument, info_span};

use crate::{
    in_memory_transport,
    machine_cursor::{MachineCursorClient, MachineCursorServer, transition::tiebreak},
};

use super::final_endpoint::FinalEndpoint;

pub use super::prelude::*;

async fn server(
    request: Option<Request>,
    conn: in_memory_transport::Connection<u8>,
) -> FinalEndpoint<crate::state::role::Server> {
    let cursor = MachineCursorServer::<Entrypoint, _>::new(conn);
    let (processor, requester) = cursor.into_parts(server::Method);
    let (to_sacrifice, to_processor_transition) = processor.handle_transition_request();

    let out: FinalEndpoint<_> = if let Some(request) = request {
        match tiebreak::from_processor_to_completion(
            to_processor_transition,
            to_sacrifice,
            requester.request_transition::<client::Method>(request),
        )
        .await
        .unwrap()
        {
            tiebreak::TiebreakResult::ProcessorWon(finalize_processor_transition) => {
                let (wrapper, transition) = finalize_processor_transition.extract_res();
                let res = transition.finish(wrapper).await.unwrap();
                res.into()
            }
            tiebreak::TiebreakResult::RequesterWon(finalize_requester_transition) => {
                let (wrapper, transition) = finalize_requester_transition.extract_res();
                let res = transition.finish(wrapper).await.unwrap();
                res.into()
            }
        }
    } else {
        let processor_transition = to_processor_transition.await.unwrap();
        let processor_transition = processor_transition
            .next_with_requester(requester)
            .await
            .unwrap();
        let (res, processor_transition) = processor_transition.extract_res();
        processor_transition.finish(res).into()
    };
    out
}

async fn client(
    request: Option<Request>,
    conn: in_memory_transport::Connection<u8>,
) -> FinalEndpoint<crate::state::role::Client> {
    let cursor = MachineCursorClient::<Entrypoint, _>::new(conn);
    let (processor, requester) = cursor.into_parts(client::Method);
    let (to_sacrifice, to_processor_transition) = processor.handle_transition_request();

    let out: FinalEndpoint<_> = if let Some(request) = request {
        match tiebreak::from_processor_to_completion(
            to_processor_transition,
            to_sacrifice,
            requester.request_transition::<server::Method>(request),
        )
        .await
        .unwrap()
        {
            tiebreak::TiebreakResult::ProcessorWon(finalize_processor_transition) => {
                let (wrapper, transition) = finalize_processor_transition.extract_res();
                transition.finish(wrapper).await.unwrap().into()
            }
            tiebreak::TiebreakResult::RequesterWon(finalize_requester_transition) => {
                let (wrapper, transition) = finalize_requester_transition.extract_res();
                transition.finish(wrapper).await.unwrap().into()
            }
        }
    } else {
        let processor_transition = to_processor_transition.await.unwrap();
        let processor_transition = processor_transition
            .next_with_requester(requester)
            .await
            .unwrap();
        let (res, processor_transition) = processor_transition.extract_res();
        processor_transition.finish(res).into()
    };
    out
}

use hegel::generators as gs;

#[hegel::composite]
fn generate_request(tc: TestCase) -> Option<Request> {
    if tc.draw(gs::booleans()) {
        None
    } else {
        let ms = tc
            .draw(gs::optional(gs::integers().max_value(10000)))
            .map(|v| Duration::from_millis(v));
        Some(Request {
            priority: tc.draw(gs::booleans()),
            sleep: ms,
        })
    }
}

#[tokio::test(start_paused = true)]
#[hegel::test(test_cases = 1000)]
async fn fuzz_from_processor_to_completion(tc: TestCase) {
    let request_server: Option<Request> = tc.draw(generate_request());
    let request_client: Option<Request> = tc.draw(generate_request());
    // one of them must transition
    tc.assume(request_client.is_some() || request_server.is_some());

    let request_priority_server = request_server.as_ref().map(|r| r.priority);
    let request_priority_client = request_client.as_ref().map(|r| r.priority);

    let network = in_memory_transport::Network::new();
    let ConnPair {
        server_conn,
        client_conn,
    } = setup_conn(0, 1, &network).await;
    let server = server(request_server, server_conn).instrument(info_span!("client"));

    let client = client(request_client, client_conn).instrument(info_span!("server"));

    let (client, server) = join(client, server).await;

    assert_eq!(client, server);
    // asserts that we tiebreak in the right direction if both are transitioning
    if let (Some(request_priority_client), Some(request_priority_server)) =
        (request_priority_client, request_priority_server)
    {
        match client {
            FinalEndpoint::Client(_) => {
                assert!(request_priority_server >= request_priority_client)
            }
            FinalEndpoint::Server(_) => assert!(request_priority_client >= request_priority_server),
        }
    }
    // asserts that we transition in the direction that was requested
    if let (Some(_), None) = (request_priority_client, request_priority_server) {
        assert!(matches!(client, FinalEndpoint::Server(_)));
    }
    if let (None, Some(_)) = (request_priority_client, request_priority_server) {
        assert!(matches!(client, FinalEndpoint::Client(_)));
    }
}

mod state;

use std::{net::SocketAddr, pin::pin, sync::Mutex, time::Duration};

use futures::future::{Either, select};
use scoped_tls::scoped_thread_local;

use crate::{
    cursor::{Cursor, requester::transition::RequestConcurrentTransitionError},
    marker::{Client, not_applicable},
};

/// Accepts the connection and then never reads from it or replies. The client's write
/// still completes because `stopped()` resolves on the peer's transport-level
/// ACK, independent of the application reading. This holds only while the
/// payload fits in the flow-control window (true for the empty request here).
async fn server(endpoint: quinn::Endpoint) -> anyhow::Result<()> {
    let _conn = super::accept_client(&endpoint).await?;
    // bounded (sim-time) so the sim can go idle once the client is done
    tokio::time::sleep(Duration::from_secs(5)).await;
    Ok(())
}

async fn client(endpoint: quinn::Endpoint, server_addr: SocketAddr) -> anyhow::Result<()> {
    let conn = super::connect_to_server(&endpoint, server_addr).await?;
    let cursor = Cursor::<state::State, Client, _>::new(conn);
    let (_processor, requester) = cursor.into_processor_and_requester(not_applicable::Handler);

    let mut loopback_buf = Vec::new();
    {
        let loopback = pin!(requester.request_loopback::<state::Loopback>((), &mut loopback_buf));
        // the server never replies, so the sleep wins and drops the loopback
        // future after its write completed but before any response was read
        match select(
            loopback,
            pin!(tokio::time::sleep(Duration::from_millis(1000))),
        )
        .await
        {
            Either::Left(_) => anyhow::bail!("loopback unexpectedly completed"),
            Either::Right(_) => {}
        }
    }

    let mut buf = Vec::new();
    let res = requester
        .request_concurrent_transition::<state::Transition>((), &mut buf)
        .await;
    let blocked = matches!(
        res,
        Err(RequestConcurrentTransitionError::AbandonedLoopback(_))
    );
    BLOCKED.with(|b| *b.lock().unwrap() = Some(blocked));
    Ok(())
}

scoped_thread_local!(static BLOCKED: Mutex<Option<bool>>);

#[test_log::test]
fn abandoned_loopback_blocks_transition() {
    let blocked = Mutex::new(None);
    BLOCKED.set(&blocked, || {
        super::harness(client, server).ok();
    });
    assert_eq!(*blocked.lock().unwrap(), Some(true));
}

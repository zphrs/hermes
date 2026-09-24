//! room server (see scratchpad/room_port_api_guide.md)
//!
//! One task (no spawning: connections are driven by a `FuturesUnordered`)
//! accepts connections; each connection loops
//! `Entrypoint -> InRoom -> Entrypoint`. Rooms are shared through [`Rooms`]
//! and users are reached through per-connection event channels, see
//! `rooms`.
//!
//! # Differences from the old example
//! - Client `Close` and a server kick both transition the client back to
//!   `Entrypoint` (that is what the state types say), so the connection is
//!   not ended by a close; it ends when the client disconnects.
//! - A `Post` is acknowledged once queued for all users, not once every
//!   user acked the notification.
//! - Rooms hold at most 10 users (old: 11 due to an off-by-one).
//! - Empty rooms are dropped instead of lingering.
//! - A failed `JoinRoom` (`RoomFull`/`UsernameTaken`) still consumes the
//!   transition credit on both sides (the rewrite has no failed-transition
//!   reply): the response carries a `Wrapper<Entrypoint>` minted from that
//!   credit, so both sides return to `Entrypoint`.
//! - The server never shuts itself down; the caller stops it.
//! - No priorities exist: races between Notify/Close/Leave are decided by
//!   whoever finalizes first.

use std::pin::pin;

use futures::{
    StreamExt,
    future::{Either, select},
    stream::FuturesUnordered,
};
use tracing::{debug, warn};

use super::states::{Entrypoint, InRoom};
use rpc_rewrite::{cursor::Cursor, marker, method::handler::root_method::RootHandler};

mod in_room;
mod join_room;
mod rooms;

use rooms::Rooms;

/// Accepts and serves connections until the endpoint is closed.
pub async fn server(endpoint: quinn::Endpoint) -> anyhow::Result<()> {
    let rooms = Rooms::default();
    let mut connections = FuturesUnordered::new();
    loop {
        let incoming = if connections.is_empty() {
            endpoint.accept().await
        } else {
            match select(pin!(endpoint.accept()), connections.next()).await {
                Either::Left((incoming, _)) => incoming,
                Either::Right((finished, _)) => {
                    if let Some(Err(error)) = finished {
                        warn!("error while handling client: {error:#}");
                    }
                    continue;
                }
            }
        };
        let Some(incoming) = incoming else {
            return Ok(());
        };
        let rooms = rooms.clone();
        connections.push(async move {
            let connection = incoming.await?;
            handle_connection(connection, rooms).await
        });
    }
}

async fn handle_connection(conn: quinn::Connection, rooms: Rooms) -> anyhow::Result<()> {
    let mut entrypoint = Cursor::<Entrypoint, marker::Server, _>::new(conn);
    loop {
        let (processor, requester) =
            entrypoint.into_processor_and_requester(RootHandler(join_room::JoinHandler {
                rooms: rooms.clone(),
            }));
        let mut buf = Vec::new();
        // Non-racing: the client is NotApplicable at Entrypoint. Errors
        // here are (almost always) the client going away.
        let (res, RootHandler(membership), credit) = match processor
            .handle_transition_request(&mut buf, requester)
            .await
        {
            Ok(ok) => ok,
            Err(error) => {
                debug!("connection ended at entrypoint: {error:?}");
                return Ok(());
            }
        };
        let (wrapper, mut membership) = match (res, membership) {
            (Ok((_, wrapper)), Some(membership)) => (wrapper, membership),
            (Err((_, back)), _) => {
                // join refused; the client is back at Entrypoint too
                entrypoint = Cursor::from_cursor_credit(credit, back);
                continue;
            }
            // the handler returns a membership exactly when the join succeeded
            (Ok(_), None) => unreachable!("successful join without membership"),
        };
        let cursor: Cursor<InRoom, marker::Server, _> = Cursor::from_cursor_credit(credit, wrapper);
        // dropping `membership` afterwards (or on error) removes the user
        entrypoint = in_room::in_room(cursor, &mut membership).await?;
    }
}

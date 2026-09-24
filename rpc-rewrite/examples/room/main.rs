//! Chat server example.
//!
//! Clients can connect to the server and join a room based on a room ID. Then
//! users in the same room can be notified of one another's messages and when
//! they join/leave the chat.
//!
//! In this case both client1 and client2 join the room "test_room_id" and post
//! "Hello, World!" to the chat. The handler for both clients are the same and
//! print out "{my_username}: {notification}" where notification is either another
//! client joining, leaving, or posting. client1 then leaves the room and
//! client2 closes it.
//!
//! The server and both clients talk over real QUIC (quinn) endpoints on
//! localhost. Each step waits for the event it depends on, so only the order
//! of the notifications between the two clients varies from run to run.
//!
//! Here's the program's stdout:
//!
//! ```ignore
//! client1 joined; existing participants: []
//! client2 joined; existing participants: [MaxLenStr("client1")]
//! client1: client2 joined
//! client1: from client1: Hello, World!
//! client2: from client1: Hello, World!
//! client1: from client2: Hello, World!
//! client2: from client2: Hello, World!
//! client2: client1 left
//! ```

mod client;
mod max_len_str;
mod server;
mod states;

use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use client::{ClientHandler, Delays, Exit, Joined, Lines, Wait};
use max_len_str::MaxLenStr;
use rpc_rewrite::quinn_transport::{client::configure_client, server::configure_server};
use tracing::{Instrument, info_span};

pub type Username = MaxLenStr<256>;
pub type RoomId = MaxLenStr<256>;

/// Waits until `line` was printed by the handler owning `lines`.
fn saw(lines: &Lines, line: &'static str) -> Wait {
    let lines = lines.clone();
    Wait::Until(Arc::new(move || {
        lines.lock().unwrap().iter().any(|l| l == line)
    }))
}

fn all(waits: Vec<Wait>) -> Wait {
    Wait::Until(Arc::new(move || waits.iter().all(Wait::ready)))
}

fn client_endpoint() -> anyhow::Result<quinn::Endpoint> {
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse()?)?;
    endpoint.set_default_client_config(configure_client()?);
    Ok(endpoint)
}

async fn join(
    endpoint: &quinn::Endpoint,
    server_addr: SocketAddr,
    name: &str,
    room: &RoomId,
) -> anyhow::Result<client::ClientCursor<states::InRoom>> {
    match client::join_room(endpoint, server_addr, &name.try_into().unwrap(), room).await? {
        Joined::InRoom(in_room) => Ok(in_room),
        Joined::Refused(error, entrypoint) => {
            drop(entrypoint);
            anyhow::bail!("{name} was refused: {error}")
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let server_endpoint = quinn::Endpoint::server(configure_server()?.0, "127.0.0.1:0".parse()?)?;
    let server_addr = server_endpoint.local_addr()?;
    // driven from this task instead of `tokio::spawn`: the server future is
    // not provably `Send` (rust-lang/rust#100013)
    let server = server::server(server_endpoint).instrument(info_span!("server"));

    let clients = async {
        let room: RoomId = "test_room_id".try_into().unwrap();
        let endpoint1 = client_endpoint()?;
        let endpoint2 = client_endpoint()?;

        // sequential so that client2 sees client1 as an existing participant
        let client1_in_room = join(&endpoint1, server_addr, "client1", &room).await?;
        let client2_in_room = join(&endpoint2, server_addr, "client2", &room).await?;

        let lines1 = Lines::default();
        let lines2 = Lines::default();
        let handler1 = ClientHandler::new("client1".try_into().unwrap(), lines1.clone());
        let handler2 = ClientHandler::new("client2".try_into().unwrap(), lines2.clone());

        let client1 = client::run_in_room(
            client1_in_room,
            handler1,
            Exit::Leave,
            Delays {
                // post once client2 is in
                before_post: saw(&lines1, "client1: client2 joined"),
                // leave once both messages were delivered to us
                before_exit: all(vec![
                    saw(&lines1, "client1: from client1: Hello, World!"),
                    saw(&lines1, "client1: from client2: Hello, World!"),
                ]),
            },
        )
        .instrument(info_span!("client1"));
        let client2 = client::run_in_room(
            client2_in_room,
            handler2,
            Exit::Close,
            Delays {
                before_post: Wait::Sleep(std::time::Duration::ZERO),
                // close once client1 has left
                before_exit: saw(&lines2, "client2: client1 left"),
            },
        )
        .instrument(info_span!("client2"));

        let (_entrypoint1, _entrypoint2) = futures::future::try_join(client1, client2)
            .await
            .context("running the clients")?;

        anyhow::Ok(())
    };

    // the server never finishes on its own: dropping it once the clients
    // are done stops it
    tokio::select! {
        res = server => res.context("server ended early"),
        res = clients => res,
    }
}

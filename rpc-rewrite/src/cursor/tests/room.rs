//! Deterministic dens-sim tests for the room example (`examples/room`).
//!
//! The example's modules are shared with this test module through `#[path]`
//! includes, so the chat code exists once; only the harness, scenario and
//! tests live here. The shared modules must therefore only refer to the
//! library through `rpc_rewrite::` (see `extern crate self` in `lib.rs`) and
//! to each other through `super::`.
#![allow(dead_code)]

#[path = "../../../examples/room/client.rs"]
pub mod client;
#[path = "../../../examples/room/max_len_str.rs"]
pub mod max_len_str;
#[path = "../../../examples/room/server/mod.rs"]
pub mod server;
#[path = "../../../examples/room/states/mod.rs"]
pub mod states;

use max_len_str::MaxLenStr;

pub type Username = MaxLenStr<256>;
pub type RoomId = MaxLenStr<256>;

use std::{
    net::SocketAddr,
    pin::pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use dens::{MachineIntoRef, Sim};
use tracing::{Instrument, info_span};

use crate::quinn_transport::create_endpoint::{dens_client, dens_server};
use client::{ClientHandler, Delays, Exit, Joined, Lines, Wait};

/// One server and two clients on the same [`dens::IpNetwork`].
fn harness<
    ServerFut: Future<Output = anyhow::Result<()>>,
    Client1Fut: Future<Output = anyhow::Result<()>>,
    Client2Fut: Future<Output = anyhow::Result<()>>,
>(
    server: impl Fn(quinn::Endpoint) -> ServerFut + 'static + Copy,
    client1: impl Fn(quinn::Endpoint, SocketAddr) -> Client1Fut + 'static + Copy,
    client2: impl Fn(quinn::Endpoint, SocketAddr) -> Client2Fut + 'static + Copy,
) -> anyhow::Result<()> {
    Sim::new_with_config(dens::sim::Config {
        tick_amount: Duration::from_millis(10),
        ..Default::default()
    })
    .enter_runtime(|| {
        let net = dens::IpNetwork::new_private_class_c().into_ref();
        let server = dens::os_mock::OsMock::new(move || {
            async move { server(dens_server(8000).await?).await }.instrument(info_span!("server"))
        });
        let (server_ipv4, _) = server.connect_to_net(net);
        let server = Sim::add_machine(server);

        let client1 = dens::os_mock::OsMock::new(move || {
            async move { client1(dens_client().await?, (server_ipv4, 8000).into()).await }
                .instrument(info_span!("client1"))
        });
        client1.connect_to_net(net);
        let client1 = client1.into_ref();

        let client2 = dens::os_mock::OsMock::new(move || {
            async move { client2(dens_client().await?, (server_ipv4, 8000).into()).await }
                .instrument(info_span!("client2"))
        });
        client2.connect_to_net(net);
        let client2 = client2.into_ref();

        let arr = [client1, client2, server];
        Sim::run_until_idle(|| arr.iter())?;
        anyhow::Ok(())
    })
}

const ROOM: &str = "test_room_id";

/// State shared by the simulated machines of one scenario. Conditions in
/// [`Wait::Until`] read it so tests sync on events rather than sleeps: the
/// default dens network has up to 1s of latency per packet plus random loss,
/// so fixed sleeps make the order of events (who joined first, who left
/// before whose post) flaky.
#[derive(Default)]
struct Ctx {
    lines: [Lines; 2],
    /// clients that are in the room
    joined: AtomicUsize,
    /// clients whose run finished
    done: AtomicUsize,
    /// `{error:?}` of refused joins
    refused: Mutex<Vec<String>>,
}

scoped_tls::scoped_thread_local!(static CTX: Ctx);

impl Ctx {
    fn saw(i: usize, line: &'static str) -> Wait {
        Wait::Until(Arc::new(move || {
            CTX.with(|c| c.lines[i].lock().unwrap().iter().any(|l| l == line))
        }))
    }
    fn joined(n: usize) -> Wait {
        Wait::Until(Arc::new(move || {
            CTX.with(|c| c.joined.load(Ordering::SeqCst) >= n)
        }))
    }
    fn refused(n: usize) -> Wait {
        Wait::Until(Arc::new(move || {
            CTX.with(|c| c.refused.lock().unwrap().len() >= n)
        }))
    }
    fn all(waits: Vec<Wait>) -> Wait {
        Wait::Until(Arc::new(move || waits.iter().all(Wait::ready)))
    }
}

struct Script {
    /// index into [`Ctx::lines`]
    idx: usize,
    name: &'static str,
    before_join: Wait,
    exit: Exit,
    delays: Delays,
}

async fn run_client(
    endpoint: quinn::Endpoint,
    server_addr: SocketAddr,
    script: Script,
) -> anyhow::Result<()> {
    let name: Username = script.name.try_into().unwrap();
    script.before_join.wait().await;
    let joined =
        client::join_room(&endpoint, server_addr, &name, &ROOM.try_into().unwrap()).await?;
    match joined {
        Joined::InRoom(in_room) => {
            CTX.with(|c| c.joined.fetch_add(1, Ordering::SeqCst));
            let lines = CTX.with(|c| c.lines[script.idx].clone());
            let handler = ClientHandler::new(name, lines);
            let _entrypoint =
                client::run_in_room(in_room, handler, script.exit, script.delays).await?;
        }
        Joined::Refused(error, _entrypoint) => {
            CTX.with(|c| c.refused.lock().unwrap().push(format!("{error:?}")));
        }
    }
    CTX.with(|c| c.done.fetch_add(1, Ordering::SeqCst));
    Ok(())
}

/// The accept loop never ends on its own; stop it once both clients are done
/// so the sim can go idle.
async fn server_until_clients_done(endpoint: quinn::Endpoint) -> anyhow::Result<()> {
    let clients_done = async {
        while CTX.with(|c| c.done.load(Ordering::SeqCst)) < 2 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    match futures::future::select(pin!(server::server(endpoint)), pin!(clients_done)).await {
        futures::future::Either::Left((res, _)) => res,
        futures::future::Either::Right(_) => Ok(()),
    }
}

fn no_wait() -> Wait {
    Wait::Sleep(Duration::ZERO)
}

fn never() -> Wait {
    Wait::Until(Arc::new(|| false))
}

fn run(script1: fn() -> Script, script2: fn() -> Script) -> Ctx {
    let ctx = Ctx::default();
    CTX.set(&ctx, || {
        harness(
            server_until_clients_done,
            move |e, a| run_client(e, a, script1()),
            move |e, a| run_client(e, a, script2()),
        )
    })
    .unwrap();
    for (i, l) in ctx.lines.iter().enumerate() {
        println!("client{} saw {:?}", i + 1, l.lock().unwrap());
    }
    assert_eq!(ctx.done.load(Ordering::SeqCst), 2, "both clients finish");
    ctx
}

fn sorted(lines: &Lines) -> Vec<String> {
    let mut l = lines.lock().unwrap().clone();
    l.sort();
    l
}

/// client1 and client2 join the same room and post; client1 leaves, then
/// client2 closes the room. Every step waits for the event it depends on, so
/// the notifications each client sees are exact (only their order varies).
#[test_log::test]
fn old_main_scenario() {
    let ctx = run(
        || Script {
            idx: 0,
            name: "client1",
            before_join: no_wait(),
            exit: Exit::Leave,
            delays: Delays {
                // post once client2 is in
                before_post: Ctx::saw(0, "client1: client2 joined"),
                // leave once both messages were delivered to us
                before_exit: Ctx::all(vec![
                    Ctx::saw(0, "client1: from client1: Hello, World!"),
                    Ctx::saw(0, "client1: from client2: Hello, World!"),
                ]),
            },
        },
        || Script {
            idx: 1,
            name: "client2",
            // join after client1 so the participant list is deterministic
            before_join: Ctx::joined(1),
            exit: Exit::Close,
            delays: Delays {
                before_post: no_wait(),
                // close once client1 has left
                before_exit: Ctx::saw(1, "client2: client1 left"),
            },
        },
    );
    assert_eq!(
        sorted(&ctx.lines[0]),
        [
            "client1: client2 joined",
            "client1: from client1: Hello, World!",
            "client1: from client2: Hello, World!",
        ]
    );
    assert_eq!(
        sorted(&ctx.lines[1]),
        [
            "client2: client1 left",
            "client2: from client1: Hello, World!",
            "client2: from client2: Hello, World!",
        ]
    );
}

/// A refused join still yields a transition credit, so the refused client is
/// back at the entrypoint.
#[test_log::test]
fn username_taken() {
    let ctx = run(
        || Script {
            idx: 0,
            name: "dupe",
            before_join: no_wait(),
            exit: Exit::Leave,
            delays: Delays {
                before_post: no_wait(),
                // stay in the room until the second join was refused
                before_exit: Ctx::refused(1),
            },
        },
        || Script {
            idx: 1,
            name: "dupe",
            before_join: Ctx::joined(1),
            exit: Exit::Leave,
            delays: Delays {
                before_post: no_wait(),
                before_exit: no_wait(),
            },
        },
    );
    assert_eq!(*ctx.refused.lock().unwrap(), ["UsernameTaken"]);
    assert_eq!(ctx.joined.load(Ordering::SeqCst), 1);
}

/// client2 closes the room while client1 is still in it (and would not leave
/// on its own): the server kicks client1 with a `ToClient` close.
#[test_log::test]
fn close_kicks_other_client() {
    let ctx = run(
        || Script {
            idx: 0,
            name: "client1",
            before_join: no_wait(),
            exit: Exit::Leave,
            delays: Delays {
                before_post: no_wait(),
                before_exit: never(),
            },
        },
        || Script {
            idx: 1,
            name: "client2",
            before_join: Ctx::joined(1),
            exit: Exit::Close,
            delays: Delays {
                before_post: no_wait(),
                // close once our own post was delivered (client1 is in the room)
                before_exit: Ctx::saw(1, "client2: from client2: Hello, World!"),
            },
        },
    );
    assert!(
        ctx.lines[0]
            .lock()
            .unwrap()
            .iter()
            .any(|l| l == "client1: client2 joined")
    );
}

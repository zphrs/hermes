//! room client (see scratchpad/room_port_api_guide.md)

use std::{
    future::pending,
    net::SocketAddr,
    pin::pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::future::{Either, select};
use tokio::sync::oneshot;
use tracing::{debug, trace};

use super::{
    RoomId, Username,
    states::{
        Entrypoint, InRoom,
        entrypoint::join_room,
        in_room::{
            Notification, Notify, close,
            from_client::{leave, post},
            to_client,
        },
    },
};
use rpc_rewrite::{
    cursor::{
        Cursor,
        requester::{RequestLoopbackError, transition::RequestConcurrentTransitionError},
        state::{Wrapper, WrapperCredit},
        transition::{RequesterOrRequesterTransition, Won, tiebreak},
    },
    marker::{self, not_applicable},
    method::{LeafHandler, ReqOf, ResOf, TransitionLeafHandler, handler::can_transition},
};

/// lines printed by the handler, shared so tests can assert on them
pub type Lines = Arc<Mutex<Vec<String>>>;

pub type ClientCursor<S> = Cursor<S, marker::Client, quinn::Connection>;

/// Outcome of [`join_room`]. A refused join still consumes the transition
/// credit, so the connection is back at the entrypoint either way.
pub enum Joined {
    InRoom(ClientCursor<InRoom>),
    Refused(join_room::Error, ClientCursor<Entrypoint>),
}

/// Connects to the server and joins `room_id`.
pub async fn join_room(
    endpoint: &quinn::Endpoint,
    server_addr: SocketAddr,
    username: &Username,
    room_id: &RoomId,
) -> anyhow::Result<Joined> {
    let conn = connect_to_server(endpoint, server_addr).await?;
    let cursor = Cursor::<Entrypoint, marker::Client, _>::new(conn);
    let (processor, requester) = cursor.into_processor_and_requester(not_applicable::Handler);
    let mut buf = Vec::new();
    let (res, credit) = requester
        .request_transition::<join_room::JoinRoom>(
            join_room::Req {
                room_id: room_id.clone(),
                username: username.clone(),
            },
            &mut buf,
            processor,
        )
        .await?;
    match res {
        Ok((users, wrapper)) => {
            println!("{username} joined; existing participants: {users:?}");
            Ok(Joined::InRoom(Cursor::from_cursor_credit(credit, wrapper)))
        }
        Err((error, back)) => {
            println!("{username} refused: {error}");
            Ok(Joined::Refused(
                error,
                Cursor::from_cursor_credit(credit, back),
            ))
        }
    }
}

#[derive(Clone)]
pub struct ClientHandler {
    pub me: Username,
    pub lines: Lines,
}

impl ClientHandler {
    pub fn new(me: Username, lines: Lines) -> Self {
        Self { me, lines }
    }
}

struct NotifyHandler {
    me: Username,
    lines: Lines,
}

impl LeafHandler<Notify> for NotifyHandler {
    async fn handle<'a>(&mut self, request: ReqOf<'a, Notify>) -> ResOf<'a, Notify> {
        let line = match request {
            Notification::Join(joiner) => format!("{}: {joiner} joined", self.me),
            Notification::Left(leaver) => format!("{}: {leaver} left", self.me),
            Notification::Mesg(message) => format!("{}: {message}", self.me),
        };
        println!("{line}");
        self.lines.lock().unwrap().push(line);
    }
}

struct CloseHandler;

impl TransitionLeafHandler<close::Method> for CloseHandler {
    type NextHandler = not_applicable::Handler;

    async fn handle_transition<'a>(
        self,
        (): ReqOf<'a, close::Method>,
        wrapper_credit: WrapperCredit<close::Method>,
    ) -> (ResOf<'a, close::Method>, Self::NextHandler) {
        (wrapper_credit.into(), not_applicable::Handler)
    }
}

impl can_transition::BranchHandler<to_client::ToClient> for ClientHandler {
    type NextHandler = not_applicable::Handler;

    async fn handle_possible_transition<
        'a,
        R: rpc_rewrite::method::Replier<to_client::ToClient>
            + rpc_rewrite::method::replier::loopback::Replier<to_client::ToClient>
            + rpc_rewrite::method::replier::transition::Replier<to_client::ToClient>,
    >(
        self,
        request: ReqOf<'a, to_client::ToClient>,
        replier: R,
    ) -> Result<
        (
            R::Receipt<ResOf<'a, to_client::ToClient>>,
            Option<Self::NextHandler>,
        ),
        R::Error,
    > {
        Ok(match request {
            to_client::Req::Notify(request) => {
                let mut leaf = NotifyHandler {
                    me: self.me,
                    lines: self.lines,
                };
                (replier.reply_with_leaf(request, &mut leaf).await?, None)
            }
            to_client::Req::Close(request) => {
                let (receipt, next) = replier.transition_with_leaf(request, CloseHandler).await?;
                (receipt, Some(next))
            }
        })
    }
}

/// How the client leaves the room after posting.
#[derive(Clone, Copy, Debug)]
pub enum Exit {
    /// leave, letting the room live on
    Leave,
    /// close the room for everyone
    Close,
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    Loopback(#[from] RequestLoopbackError<quinn::Connection>),
    #[error(transparent)]
    Transition(#[from] RequestConcurrentTransitionError<quinn::Connection>),
}

/// What to wait for before a step. Tests use `Until` rather than fixed sleeps
/// so they do not depend on the simulated network's latency and loss.
#[derive(Clone)]
pub enum Wait {
    Sleep(Duration),
    /// polls (in sim-time) until the condition holds
    Until(Arc<dyn Fn() -> bool>),
}

impl Wait {
    pub fn ready(&self) -> bool {
        match self {
            Wait::Sleep(_) => true,
            Wait::Until(cond) => cond(),
        }
    }

    pub async fn wait(&self) {
        match self {
            Wait::Sleep(d) => tokio::time::sleep(*d).await,
            Wait::Until(cond) => {
                while !cond() {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
    }
}

/// Waits that let a test line up the clients.
#[derive(Clone)]
pub struct Delays {
    pub before_post: Wait,
    pub before_exit: Wait,
}

/// Posts "Hello, World!" then requests `M` (leave or close) while handling
/// server notifications, racing the server's own `Close` via the tiebreak.
/// Either way we end up at the entrypoint.
macro_rules! race_exit {
    ($name:ident, $M:ty) => {
        async fn $name(
            cursor: ClientCursor<InRoom>,
            handler: ClientHandler,
            delays: Delays,
        ) -> anyhow::Result<ClientCursor<Entrypoint>> {
            let (processor, requester) = cursor.into_processor_and_requester(handler);
            let mut read_into = Vec::new();
            let mut buf = Vec::new();
            let mut processor_fut =
                pin!(processor.handle_hybrid_concurrent_transition_requests(&mut buf));
            let mut post_buf = Vec::new();
            let need_requester = tokio::sync::Notify::new();
            let to_requester = oneshot::channel();
            let requester_fut = async {
                // whenever the server's transition wins while we wait, hand the
                // requester back (only between loopbacks, see the guide)
                if let Either::Left(_) = select(
                    pin!(need_requester.notified()),
                    pin!(delays.before_post.wait()),
                )
                .await
                {
                    to_requester.0.send(requester).ok().unwrap();
                    return pending().await;
                }
                // always finish loopbacks: dropping one poisons the next transition
                requester
                    .request_loopback::<post::Method>(
                        "Hello, World!".try_into().unwrap(),
                        &mut post_buf,
                    )
                    .await?;
                debug!("sent hello world");
                if let Either::Left(_) = select(
                    pin!(need_requester.notified()),
                    pin!(delays.before_exit.wait()),
                )
                .await
                {
                    to_requester.0.send(requester).ok().unwrap();
                    return pending().await;
                }
                Ok::<_, RunError>(
                    requester
                        .request_concurrent_transition::<$M>((), &mut read_into)
                        .await?,
                )
            };
            let mut requester_fut = pin!(requester_fut);
            let (won, credit) = match tiebreak(&mut processor_fut, &mut requester_fut).await? {
                tiebreak::With::Processor(processor_transition) => {
                    trace!("server's close won");
                    need_requester.notify_one();
                    processor_transition
                        .next(async move {
                            anyhow::Ok(match select(requester_fut, to_requester.1).await {
                                Either::Left((rt, _)) => {
                                    RequesterOrRequesterTransition::RequesterTransition(rt?)
                                }
                                Either::Right((r, _)) => {
                                    RequesterOrRequesterTransition::Requester(r?)
                                }
                            })
                        })
                        .await?
                }
                tiebreak::With::Requester(requester_transition) => {
                    trace!("our transition won");
                    requester_transition.next(processor_fut).await?
                }
            };
            let wrapper: Wrapper<Entrypoint> = match won {
                Won::Processor { res, .. } => match res {
                    to_client::Res::Close(wrapper) => wrapper,
                    to_client::Res::Notify(()) => unreachable!("a notification cannot transition"),
                },
                Won::Requester { res } => res,
            };
            Ok(Cursor::from_cursor_credit(credit, wrapper))
        }
    };
}

race_exit!(race_leave, leave::Method);
race_exit!(race_close, close::Method);

pub async fn run_in_room(
    cursor: ClientCursor<InRoom>,
    handler: ClientHandler,
    exit: Exit,
    delays: Delays,
) -> anyhow::Result<ClientCursor<Entrypoint>> {
    match exit {
        Exit::Leave => race_leave(cursor, handler, delays).await,
        Exit::Close => race_close(cursor, handler, delays).await,
    }
}

async fn connect_to_server(
    endpoint: &quinn::Endpoint,
    address: impl Into<SocketAddr>,
) -> anyhow::Result<quinn::Connection> {
    Ok(endpoint.connect(address.into(), "server.invalid")?.await?)
}

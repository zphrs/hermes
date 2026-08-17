use futures::{FutureExt, select};
use max_sized_vec::MaxSizedVec;

use rpc::{
    Transport as _, in_memory_transport,
    machine_cursor::{
        self, MachineCursorServer,
        transition::{
            processor::{self, ProcessorTransition},
            tiebreak,
        },
    },
    method::{Ancestor, ancestors},
    state::{Has, role},
    traits::{HandleError, HandlerResult},
    transport::Incoming,
};
use std::{
    collections::HashMap,
    convert::Infallible,
    future::pending,
    sync::{Arc, Mutex},
};
use tracing::{Instrument, debug, span, trace, warn};

use crate::{
    RoomId, Username,
    server::room::{AddUserPermit, Room},
    states::{
        Entrypoint,
        entrypoint::join_room::{self},
        in_room::{
            self, close,
            from_client::{self, leave, loopback, post},
        },
    },
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("room closed")]
    RoomClosed,
}

pub struct User {
    name: Username,
    requester: rpc::machine_cursor::Requester<
        in_room::InRoom,
        role::Server,
        <in_room::InRoom as rpc::State>::ClientHandles,
        in_memory_transport::Connection<&'static str>,
    >,
    #[allow(clippy::type_complexity)]
    room_closing_notification: tokio::sync::oneshot::Sender<
        machine_cursor::transition::requester::RequesterTransition<
            in_room::InRoom,
            machine_cursor::transition::RequestTransition<
                in_room::ToClient,
                close::Method,
                role::Server,
                in_memory_transport::Connection<&'static str>,
            >,
        >,
    >,
}

impl User {
    #[allow(clippy::type_complexity)]
    pub fn new(
        name: Username,
        requester: rpc::machine_cursor::Requester<
            in_room::InRoom,
            role::Server,
            <in_room::InRoom as rpc::State>::ClientHandles,
            in_memory_transport::Connection<&'static str>,
        >,
        room_closing_notification: tokio::sync::oneshot::Sender<
            machine_cursor::transition::requester::RequesterTransition<
                in_room::InRoom,
                machine_cursor::transition::RequestTransition<
                    in_room::ToClient,
                    close::Method,
                    role::Server,
                    in_memory_transport::Connection<&'static str>,
                >,
            >,
        >,
    ) -> User {
        Self {
            name,
            requester,
            room_closing_notification,
        }
    }

    pub async fn kick_user(self) {
        let requester_transition: machine_cursor::transition::requester::RequesterTransition<
            in_room::InRoom,
            machine_cursor::transition::RequestTransition<
                in_room::ToClient,
                close::Method,
                role::Server,
                in_memory_transport::Connection<&str>,
            >,
        > = self
            .requester
            .request_transition::<in_room::close::Method>(());
        self.room_closing_notification
            .send(requester_transition)
            .ok()
            .expect(
                "should be the first time we send because we only send \
                    in functions that take ownership of self",
            );
    }
}

pub mod room {
    use std::{collections::HashMap, sync::Arc};

    use arrayvec::ArrayVec;
    use futures::{StreamExt as _, TryStreamExt, stream::FuturesUnordered};

    use tracing::trace;

    use crate::{
        RoomId, Username,
        server::User,
        states::{
            entrypoint::join_room,
            in_room::{Notification, Notify},
        },
    };

    pub struct Room {
        pub id: RoomId,
        users: HashMap<Username, User>,
        pending_user_count: usize,
    }

    pub struct AddUserPermit(Username);

    impl AddUserPermit {
        pub fn username(&self) -> &Username {
            &self.0
        }
    }

    impl Room {
        pub fn new(id: RoomId) -> Self {
            Self {
                id,
                users: HashMap::new(),
                pending_user_count: 0,
            }
        }

        pub async fn add_user(&mut self, user: User, _permit: AddUserPermit) {
            self.pending_user_count -= 1;
            self.notify([Notification::Join(user.name.clone())]).await;
            self.users.insert(user.name.clone(), user);
        }

        pub fn add_user_permit(
            &mut self,
            username: Username,
        ) -> Result<(ArrayVec<Username, 10>, AddUserPermit), join_room::Error> {
            if self.users.len() + self.pending_user_count > 10 {
                Err(join_room::Error::RoomFull)?
            }
            if self.users.contains_key(&username) {
                Err(join_room::Error::UsernameTaken)?
            }

            self.pending_user_count += 1;

            Ok((
                ArrayVec::from_iter(self.users.keys().cloned()),
                AddUserPermit(username),
            ))
        }

        pub async fn close(self) {
            let kick_users = FuturesUnordered::new();
            for user in self.users.into_values() {
                kick_users.push(user.kick_user());
            }
            kick_users.collect::<()>().await
        }

        pub async fn remove_user(&mut self, username: &Username) -> Option<User> {
            let removed = self.users.remove(username);

            self.notify([Notification::Left(username.clone())]).await;

            removed
        }

        async fn notify<const CAP: usize>(
            &mut self,
            notifications: impl Into<ArrayVec<Notification, CAP>>,
        ) {
            let mut set = FuturesUnordered::new();
            let notifications = Arc::new(notifications.into());
            for user in self.users.values() {
                let notifications = notifications.clone();
                set.push(async move {
                    let set = FuturesUnordered::new();
                    for notification in notifications.iter() {
                        trace!("notifying {} {:?}", user.name, notification);
                        set.push(
                            user.requester
                                .request_loopback::<Notify>(notification.clone()),
                        );
                    }

                    set.try_collect::<()>().await.map_err(|_e| &user.name)?;
                    trace!("finished notifying {}", user.name);
                    Ok::<(), &Username>(())
                });
            }
            let mut to_remove = Vec::new();
            while let Some(next) = set.next().await {
                match next {
                    Ok(()) => continue,
                    Err(name) => to_remove.push(name.clone()),
                };
            }
            debug_assert!(
                set.is_empty(),
                "the loop above fully consumes all pending futures"
            );
            drop(set);
            if to_remove.is_empty() {
                return;
            }

            for username in to_remove.iter() {
                self.users.remove(username);
            }

            let to_remove_notifications = to_remove
                .into_iter()
                .map(Notification::Left)
                .collect::<ArrayVec<_, 10>>();
            Box::pin(self.notify(to_remove_notifications)).await;
        }

        pub async fn send_message(
            &mut self,
            username: crate::max_len_str::MaxLenStr<256>,
            body: crate::max_len_str::MaxLenStr<1024>,
        ) {
            let message = crate::states::in_room::Message {
                from: username,
                body,
            };
            self.notify([Notification::Mesg(message)]).await
        }
    }
}

type Rooms = Arc<Mutex<HashMap<Username, Arc<tokio::sync::Mutex<Option<Room>>>>>>;

pub struct JoinRoomHandler {
    rooms: Rooms,
    room_lock: Option<tokio::sync::OwnedMutexGuard<Option<Room>>>,
    add_user_permit: Option<AddUserPermit>,
}

impl JoinRoomHandler {
    pub fn new(rooms: Rooms) -> Self {
        Self {
            rooms,
            room_lock: None,
            add_user_permit: None,
        }
    }
}

impl<RootMethod: Ancestor<join_room::JoinRoom>> rpc::Handler<RootMethod, join_room::JoinRoom>
    for JoinRoomHandler
{
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RootMethod, join_room::JoinRoom>>(
        &mut self,
        replier: Replier,
        value: rpc::ReqOf<join_room::JoinRoom>,
    ) -> HandlerResult<RootMethod, join_room::JoinRoom, Replier, Self::Error> {
        // async block to Try the result of add_user_permit
        let res: rpc::ResOf<join_room::JoinRoom> = async {
            let entry = self
                .rooms
                .lock()
                .unwrap()
                .entry(value.room_id.clone())
                .or_insert(Arc::new(tokio::sync::Mutex::new(None)))
                .clone();
            let mut room_lock = entry.lock_owned().await;

            let room = &mut *room_lock.get_or_insert(Room::new(value.room_id));
            let (out, permit) = room.add_user_permit(value.username)?;
            self.add_user_permit = Some(permit);
            self.room_lock = Some(room_lock);

            let new_state = replier.new_wrapper();
            Ok((MaxSizedVec::from_inner(out), new_state))
        }
        .await;
        replier.reply(res).await
    }
}

pub struct FromClientHandler {
    rooms: Rooms,
    room: Arc<tokio::sync::Mutex<Option<Room>>>,
    username: Username,
    removed_user: Option<User>,
}

impl FromClientHandler {
    pub fn to_loopback_handler(&self) -> FromClientLoopbackHandler {
        let Self { room, username, .. } = self;
        FromClientLoopbackHandler {
            room: room.clone(),
            username: username.clone(),
        }
    }
}

#[derive(Clone)]
pub struct FromClientLoopbackHandler {
    room: Arc<tokio::sync::Mutex<Option<Room>>>,
    username: Username,
}

impl FromClientHandler {
    pub fn new(
        rooms: Rooms,
        room: Arc<tokio::sync::Mutex<Option<Room>>>,
        room_id: &RoomId,
        username: Username,
    ) -> Self {
        {
            debug_assert!(rooms.lock().unwrap().contains_key(room_id));
        }
        Self {
            rooms,
            room,
            username,
            removed_user: None,
        }
    }

    async fn block_if_room_closed<ReplierError, Receipt>(
        result: Result<Receipt, HandleError<ReplierError, Error>>,
    ) -> Result<Receipt, HandleError<ReplierError, Infallible>> {
        match result {
            // if room is closed then we should just wait for the room
            // to finish closing via the requester and not resolve the request
            Err(HandleError::Handler(Error::RoomClosed)) => pending().await,
            Err(HandleError::Replier(replier)) => Err(HandleError::Replier(replier)),
            Ok(out) => Ok(out),
        }
    }
}

pub trait LoopbackAncestors: ancestors::Two<loopback::Method, post::Method> {}

impl<T: ancestors::Two<loopback::Method, post::Method>> LoopbackAncestors for T {}

impl<RootMethod: LoopbackAncestors> rpc::Handler<RootMethod, loopback::Method>
    for FromClientLoopbackHandler
{
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RootMethod, loopback::Method>>(
        &mut self,
        replier: Replier,
        value: rpc::ReqOf<loopback::Method>,
    ) -> HandlerResult<RootMethod, loopback::Method, Replier, Self::Error> {
        let result = async move {
            match value {
                loopback::Req::Post(post) => {
                    replier
                        .reply_with::<post::Method, _>(self, post, loopback::Res::Post)
                        .await
                }
            }
        }
        .await;
        FromClientHandler::block_if_room_closed(result).await
    }
}

impl<RootMethod: Ancestor<post::Method>> rpc::Handler<RootMethod, post::Method>
    for FromClientLoopbackHandler
{
    type Error = Error;

    async fn handle<Replier: rpc::ReplyHelper<RootMethod, post::Method>>(
        &mut self,
        replier: Replier,
        body: rpc::ReqOf<post::Method>,
    ) -> HandlerResult<RootMethod, post::Method, Replier, Self::Error> {
        let mut room_lock = self.room.lock().await;
        let room = room_lock.as_mut().ok_or(Error::RoomClosed)?;

        room.send_message(self.username.clone(), body).await;

        replier.reply(()).await
    }
}

impl<RootMethod: Ancestor<close::Method>> rpc::Handler<RootMethod, close::Method>
    for FromClientHandler
{
    type Error = Error;

    async fn handle<Replier: rpc::ReplyHelper<RootMethod, close::Method>>(
        &mut self,
        replier: Replier,
        (): rpc::ReqOf<close::Method>,
    ) -> HandlerResult<RootMethod, close::Method, Replier, Self::Error> {
        // either we trigger closing the room or the room is already closed
        let mut room_lock = self.room.lock().await;
        let room = room_lock.take().ok_or(Error::RoomClosed)?;
        drop(room_lock);
        self.rooms.lock().unwrap().remove(&room.id);
        room.close().await;
        let wrapper = replier.new_wrapper();
        replier.reply(wrapper).await
    }
}

impl<RootMethod: Ancestor<leave::Method>> rpc::Handler<RootMethod, leave::Method>
    for FromClientHandler
{
    type Error = Error;

    async fn handle<Replier: rpc::ReplyHelper<RootMethod, leave::Method>>(
        &mut self,
        replier: Replier,
        (): rpc::ReqOf<leave::Method>,
    ) -> HandlerResult<RootMethod, leave::Method, Replier, Self::Error> {
        {
            let mut room_lock = self.room.lock().await;
            let room = room_lock.as_mut().ok_or(Error::RoomClosed)?;

            let me = room.remove_user(&self.username).await.unwrap();
            debug!("removed {}", me.name);
            self.removed_user = Some(me);
        } // closure here to make sure we drop room_lock before replying
        let wrapper = replier.new_wrapper();
        replier.reply(wrapper).await
    }
}

impl<
    RootMethod: ancestors::Three<from_client::Method, close::Method, leave::Method> + LoopbackAncestors,
> rpc::Handler<RootMethod, from_client::Method> for FromClientHandler
{
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RootMethod, from_client::Method>>(
        &mut self,
        replier: Replier,
        value: rpc::ReqOf<from_client::Method>,
    ) -> HandlerResult<RootMethod, from_client::Method, Replier, Self::Error> {
        match value {
            from_client::Req::Loopback(request) => {
                let mut loopback_handler = self.to_loopback_handler();
                replier
                    .reply_with::<loopback::Method, FromClientLoopbackHandler>(
                        &mut loopback_handler,
                        request,
                        from_client::Res::Loopback,
                    )
                    .await
            }
            from_client::Req::Close(req) => {
                Self::block_if_room_closed(
                    replier
                        .reply_with::<close::Method, _>(self, req, from_client::Res::Close)
                        .await,
                )
                .await
            }
            from_client::Req::Leave => {
                Self::block_if_room_closed(
                    replier
                        .reply_with::<leave::Method, _>(self, (), from_client::Res::Leave)
                        .await,
                )
                .await
            }
        }
    }
}

pub async fn handle_incoming(
    conn: in_memory_transport::Connection<&'static str>,
    rooms: Rooms,
) -> anyhow::Result<()> {
    let mut cursor = MachineCursorServer::<Entrypoint, _>::new(conn);
    loop {
        let rooms = rooms.clone();
        let mut handler = JoinRoomHandler::new(rooms.clone());
        let (processor, requester) = cursor.into_children_with_handler(&mut handler);
        let (res, transition) = processor
            .handle_transition_request()
            .await?
            .next_with_requester(requester)
            .await?
            .extract_res();

        let mut room_mutex_guard = handler.room_lock.take().unwrap();
        let permit = handler.add_user_permit.take().unwrap();
        let room = room_mutex_guard.as_mut().unwrap();

        let room_arc = rooms.lock().unwrap().get(&room.id).unwrap().clone();

        let mut handler =
            FromClientHandler::new(rooms, room_arc, &room.id, permit.username().clone());
        let loopback_handler = handler.to_loopback_handler();
        let (processor, requester) = transition
            .finish(res.extract_wrapper())
            .into_children_with_handler(&mut handler);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        {
            room.add_user(
                User::new(permit.username().clone(), requester, sender),
                permit,
            )
            .await;
        }
        drop(room_mutex_guard);
        let mut eventual_processor_transition =
            processor.handle_requests::<loopback::Method, _>(loopback_handler);
        debug!("server handling requests");
        cursor = select! {
            processor_transition = eventual_processor_transition => {
                drop(eventual_processor_transition);
                trace!("handling processor transition");
                handle_processor_transition(processor_transition, handler).await?
            },
            receiver_transition = receiver.fuse() => handle_receiver_transition(receiver_transition?, eventual_processor_transition).await?,
        };
    }
}

async fn handle_receiver_transition(
    receiver: machine_cursor::transition::requester::RequesterTransition<
        in_room::InRoom,
        machine_cursor::transition::RequestTransition<
            in_room::ToClient,
            close::Method,
            role::Server,
            in_memory_transport::Connection<&'static str>,
        >,
    >,
    eventual_processor_transition: machine_cursor::EventualTransitionRequest<
        impl Future<
            Output = Result<
                ProcessorTransition<
                    processor::Entrypoint<
                        in_room::InRoom,
                        from_client::Method,
                        role::Server,
                        in_memory_transport::Connection<&'static str>,
                    >,
                >,
                machine_cursor::processor::MultipleRequestsError<
                    <in_memory_transport::Connection<&'static str> as rpc::transport::Client>::Error,
                    <FromClientLoopbackHandler as rpc::Handler<
                        from_client::Method,
                        loopback::Method,
                    >>::Error,
                    <FromClientHandler as rpc::Handler<
                        from_client::Method,
                        from_client::Method,
                    >>::Error,
                >,
            >,
        >,
    >,
) -> anyhow::Result<
    rpc::MachineCursor<Entrypoint, in_memory_transport::Connection<&'static str>, role::Server>,
> {
    let tiebreak_result = tiebreak::between_potential_processor_and_known_requester_transition(
        eventual_processor_transition,
        receiver,
    )
    .await?;
    match tiebreak_result {
        tiebreak::TiebreakResult::ProcessorWon(finalize_processor_transition) => {
            let (res, finalize) = finalize_processor_transition.extract_res();
            Ok(finalize.finish(res.extract_wrapper()).await?)
        }
        tiebreak::TiebreakResult::RequesterWon(finalize_requester_transition) => {
            let (res, finalize) = finalize_requester_transition.extract_res();
            Ok(finalize.finish(res.extract_wrapper()).await?)
        }
    }
}

#[allow(clippy::type_complexity)]
async fn handle_processor_transition(
    processor_transition_result: Result<
        ProcessorTransition<
            processor::Entrypoint<
                in_room::InRoom,
                from_client::Method,
                role::Server,
                in_memory_transport::Connection<&str>,
            >,
        >,
        machine_cursor::processor::MultipleRequestsError<std::io::Error, Infallible, Infallible>,
    >,
    mut handler: FromClientHandler,
) -> anyhow::Result<
    rpc::MachineCursor<Entrypoint, in_memory_transport::Connection<&str>, role::Server>,
> {
    let processor_transition = processor_transition_result?;
    // either they called a close or a leave, either way we remove them
    // from the room
    if let Some(removed_user) = handler.removed_user.take() {
        let (res, processor_transition) = processor_transition
            .next_with_requester(removed_user.requester)
            .await?
            .extract_res();

        Ok(processor_transition.finish(res.extract_wrapper()))
    } else {
        unreachable!()
    }
}

pub async fn server(transport: in_memory_transport::MemoryTransport<&'static str>) {
    let rooms: Rooms = Rooms::default();
    loop {
        let Ok(incoming) = transport.accept().await;
        let conn = incoming.accept().await.unwrap();
        let rooms = rooms.clone();

        warn!("value");

        tokio::spawn(
            async move {
                if let Err(err) = handle_incoming(conn, rooms.clone()).await {
                    warn!("error while handling client: {err}");
                };
            }
            .instrument(span::Span::current()),
        );
    }
}

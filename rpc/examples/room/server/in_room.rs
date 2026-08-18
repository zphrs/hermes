use rpc::traits::HandleError;
use tracing::debug;

use crate::RoomId;
use crate::server::user::{self, Error};
use crate::states::in_room::from_client::{leave, loopback, post};
use crate::states::in_room::{close, from_client};

use rpc::method::{Ancestor, ancestors};

use rpc::traits::HandlerResult;

use std::convert::Infallible;
use std::future::pending;

use super::user::User;

use crate::Username;

use crate::server::room::Room;

use std::sync::Arc;

use super::Rooms;

pub struct FromClientHandler {
    pub(crate) rooms: Rooms,
    pub(crate) room: Arc<tokio::sync::Mutex<Option<Room>>>,
    pub(crate) username: Username,
    pub(crate) removed_user: Option<User>,
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
    pub(crate) room: Arc<tokio::sync::Mutex<Option<Room>>>,
    pub(crate) username: Username,
}

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

    pub(crate) async fn block_if_room_closed<ReplierError, Receipt>(
        result: Result<Receipt, HandleError<ReplierError, Error>>,
    ) -> Result<Receipt, HandleError<ReplierError, Infallible>> {
        match result {
            // if room is closed then we should just wait for the room
            // to finish closing via the requester and not resolve the request
            Err(HandleError::Handler(user::Error::RoomClosed)) => pending().await,
            Err(HandleError::Replier(replier)) => Err(HandleError::Replier(replier)),
            Ok(out) => Ok(out),
        }
    }
}

pub trait LoopbackAncestors: ancestors::Two<loopback::Method, post::Method> {}

impl<T: ancestors::Two<loopback::Method, post::Method>> LoopbackAncestors for T {}

impl<RootMethod: Ancestor<close::Method>> rpc::Handler<RootMethod, close::Method>
    for FromClientHandler
{
    type Error = user::Error;

    async fn handle<Replier: rpc::ReplyHelper<RootMethod, close::Method>>(
        &mut self,
        replier: Replier,
        (): rpc::ReqOf<close::Method>,
    ) -> HandlerResult<RootMethod, close::Method, Replier, Self::Error> {
        // either we trigger closing the room or the room is already closed
        let mut room_lock = self.room.lock().await;
        let Some(room) = room_lock.take() else {
            let wrapper = replier.new_wrapper();
            return replier.reply(wrapper).await;
        };
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
    type Error = user::Error;

    async fn handle<Replier: rpc::ReplyHelper<RootMethod, leave::Method>>(
        &mut self,
        replier: Replier,
        (): rpc::ReqOf<leave::Method>,
    ) -> HandlerResult<RootMethod, leave::Method, Replier, Self::Error> {
        {
            let mut room_lock = self.room.lock().await;
            let Some(room) = room_lock.as_mut() else {
                let wrapper = replier.new_wrapper();
                return replier.reply(wrapper).await;
            };

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

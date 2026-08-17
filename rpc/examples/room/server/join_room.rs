use max_sized_vec::MaxSizedVec;

use std::sync::Arc;

use rpc::traits::HandlerResult;

use std::convert::Infallible;

use rpc::method::Ancestor;

use crate::server::room::AddUserPermit;

use crate::server::room::Room;
use crate::states::entrypoint::join_room;

use super::Rooms;

pub struct JoinRoomHandler {
    pub(crate) rooms: Rooms,
    pub(crate) room_lock: Option<tokio::sync::OwnedMutexGuard<Option<Room>>>,
    pub(crate) add_user_permit: Option<AddUserPermit>,
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

use rpc::{
    in_memory_transport,
    machine_cursor::{self, transition::requester::RequesterTransitionServerEntrypoint},
    state::role,
};

use crate::{
    Username,
    states::in_room::{self, close},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("room closed")]
    RoomClosed,
}

pub struct User {
    pub(crate) name: Username,
    pub(crate) requester: rpc::machine_cursor::Requester<
        in_room::InRoom,
        role::Server,
        <in_room::InRoom as rpc::State>::ClientHandles,
        in_memory_transport::Connection<&'static str>,
    >,
    pub(crate) room_closing_notification: tokio::sync::oneshot::Sender<
        RequesterTransitionServerEntrypoint<
            in_room::InRoom,
            close::Method,
            in_memory_transport::Connection<&'static str>,
        >,
    >,
}

impl User {
    pub fn new(
        name: Username,
        requester: rpc::machine_cursor::Requester<
            in_room::InRoom,
            role::Server,
            <in_room::InRoom as rpc::State>::ClientHandles,
            in_memory_transport::Connection<&'static str>,
        >,
        room_closing_notification: tokio::sync::oneshot::Sender<
            RequesterTransitionServerEntrypoint<
                in_room::InRoom,
                close::Method,
                in_memory_transport::Connection<&'static str>,
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
            machine_cursor::transition::StageOne<
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

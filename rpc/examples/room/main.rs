mod max_len_str;

use futures::future::select;
use max_len_str::MaxLenStr;

use rpc::{
    in_memory_transport::Network,
    machine_cursor::{MachineCursorClient, transition::tiebreak},
};
use tracing::{Instrument, debug, info_span};

use crate::{
    client::ClientHandler,
    states::{
        Entrypoint,
        in_room::{
            self, Message,
            from_client::{leave, post, subscribe},
        },
    },
};

pub type Username = MaxLenStr<256>;

pub type RoomId = MaxLenStr<256>;

pub mod states {
    use rpc::{define_prioritized, method::not_applicable::NotApplicable, state::priority};

    pub struct Entrypoint;

    impl rpc::State for Entrypoint {
        type ClientHandles = NotApplicable;

        type ServerHandles = entrypoint::join_room::JoinRoom;
    }

    define_prioritized!(Entrypoint, priority::server_wins);

    pub mod entrypoint {

        pub mod join_room {
            use rpc::{
                method::{can_transition, is_leaf},
                state,
            };

            use crate::{RoomId, Username, states::in_room::InRoom};

            use max_sized_vec::MaxSizedVec;

            pub struct JoinRoom;

            #[derive(
                minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen, Clone,
            )]
            pub struct Req {
                #[n(0)]
                pub room_id: RoomId,
                #[n(1)]
                pub username: Username,
            }

            #[derive(
                Debug,
                thiserror::Error,
                minicbor::Encode,
                minicbor::Decode,
                minicbor::CborLen,
                maxlen::MaxLen,
            )]
            #[repr(u8)]
            pub enum Error {
                #[n(0)]
                #[error("room full")]
                RoomFull = 0,
                #[n(1)]
                #[error("username taken")]
                UsernameTaken = 1,
            }

            impl rpc::Method for JoinRoom {
                type Req = Req;

                type Res = Result<
                    (
                        MaxSizedVec<Username, 10>,
                        state::Wrapper<crate::states::in_room::InRoom>,
                    ),
                    Error,
                >;

                type CanTransition = can_transition::True;

                type IsLeaf = is_leaf::True;
            }

            impl state::Has<crate::states::in_room::InRoom>
                for Result<(MaxSizedVec<Username, 10>, rpc::state::Wrapper<InRoom>), Error>
            {
                fn extract_wrapper(self) -> rpc::state::Wrapper<crate::states::in_room::InRoom> {
                    self.ok().unwrap().1
                }
            }
        }
    }

    pub mod in_room {
        use rpc::{
            method::{Ancestor, FromDescendant, can_transition, is_leaf},
            state::Prioritized,
        };

        use crate::{Username, max_len_str::MaxLenStr, states::in_room::from_client::subscribe};

        #[derive(
            Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen,
        )]
        pub struct Message {
            #[n(0)]
            pub from: Username,
            #[n(1)]
            pub body: MaxLenStr<1024>,
        }

        impl std::fmt::Display for Message {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                writeln!(f, "from {}: {}", self.from, self.body)
            }
        }

        #[derive(
            Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen,
        )]
        pub enum Notification {
            #[n(0)]
            Join(#[n(0)] Username),
            #[n(1)]
            Left(#[n(0)] Username),
            #[n(2)]
            Mesg(#[n(0)] Message),
        }

        impl Notification {
            pub fn flag(&self) -> u8 {
                match self {
                    Notification::Join(_) => subscribe::Flags::JOIN,
                    Notification::Left(_) => subscribe::Flags::LEFT,
                    Notification::Mesg(_) => subscribe::Flags::MESG,
                }
            }
        }

        pub mod close {
            use rpc::{
                method::{can_transition, is_leaf},
                state,
            };

            use crate::states::Entrypoint;

            pub struct Method;

            impl rpc::Method for Method {
                type Req = ();

                type Res = state::Wrapper<Entrypoint>;

                type CanTransition = can_transition::True;

                type IsLeaf = is_leaf::True;
            }
        }

        #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
        pub enum ToClientReq {
            #[n(0)]
            Notif(#[n(0)] <Notify as rpc::Method>::Req),
            #[n(1)]
            Close(#[n(0)] <close::Method as rpc::Method>::Req),
        }

        pub enum ToClientRes {
            Notif(<Notify as rpc::Method>::Res),
            Close(<close::Method as rpc::Method>::Res),
        }

        pub struct Notify;

        impl rpc::Method for Notify {
            type Req = Notification;

            type Res = ();

            type CanTransition = can_transition::False;

            type IsLeaf = is_leaf::True;
        }

        pub struct ToClient;

        impl Ancestor<Notify> for ToClient {}
        impl Ancestor<close::Method> for ToClient {}

        impl FromDescendant<close::Method> for ToClient {
            fn from_descendant_req(
                request: <close::Method as rpc::Method>::Req,
            ) -> <Self as rpc::Method>::Req {
                ToClientReq::Close(request)
            }

            fn from_descendant_res(
                result: <close::Method as rpc::Method>::Res,
            ) -> <Self as rpc::Method>::Res {
                ToClientRes::Close(result)
            }

            fn try_into_descendant_req(
                request: Self::Req,
            ) -> Result<<close::Method as rpc::Method>::Req, Self::Req> {
                match request {
                    ToClientReq::Close(child) => Ok(child),
                    other => Err(other),
                }
            }
        }

        impl FromDescendant<Notify> for ToClient {
            fn from_descendant_req(
                request: <Notify as rpc::Method>::Req,
            ) -> <Self as rpc::Method>::Req {
                ToClientReq::Notif(request)
            }

            fn from_descendant_res(
                result: <Notify as rpc::Method>::Res,
            ) -> <Self as rpc::Method>::Res {
                ToClientRes::Notif(result)
            }

            fn try_into_descendant_req(
                request: Self::Req,
            ) -> Result<<Notify as rpc::Method>::Req, Self::Req> {
                match request {
                    ToClientReq::Notif(child) => Ok(child),
                    other => Err(other),
                }
            }
        }

        impl rpc::Method for ToClient {
            type Req = ToClientReq;

            type Res = ToClientRes;

            type CanTransition = can_transition::True;

            type IsLeaf = is_leaf::False;
        }

        pub mod from_client {
            use rpc::{
                method::{Ancestor, FromDescendant, can_transition, is_leaf},
                state,
            };

            use crate::states::in_room::{close, from_client};

            pub mod subscribe {
                use rpc::method::{can_transition, is_leaf};

                #[derive(
                    Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen,
                )]
                pub struct Flags {
                    #[n(0)]
                    flags: u8,
                }

                impl Flags {
                    pub const JOIN: u8 = 0b00000001;
                    pub const LEFT: u8 = 0b00000010;
                    pub const MESG: u8 = 0b00000100;

                    pub fn empty() -> Self {
                        Self { flags: 0 }
                    }

                    pub fn all() -> Self {
                        Self {
                            flags: Self::JOIN | Self::LEFT | Self::MESG,
                        }
                    }

                    pub fn set(&mut self, flag: u8) -> &mut Self {
                        self.flags |= flag;
                        self
                    }

                    pub fn unset(&mut self, flag: u8) -> &mut Self {
                        self.flags &= !flag;
                        self
                    }

                    pub fn check(&self, flag: u8) -> bool {
                        (self.flags & flag) != 0
                    }
                }

                pub struct Method;

                impl rpc::Method for Method {
                    type Req = Flags;

                    type Res = ();

                    type CanTransition = can_transition::False;

                    type IsLeaf = is_leaf::True;
                }
            }
            pub mod post {
                use rpc::method::{can_transition, is_leaf};

                use super::super::Message;

                pub struct Method;

                impl rpc::Method for Method {
                    type Req = Message;

                    type Res = ();

                    type CanTransition = can_transition::False;

                    type IsLeaf = is_leaf::True;
                }
            }
            pub mod loopback {
                use rpc::method::{Ancestor, FromDescendant, can_transition, is_leaf};

                use super::{post, subscribe};

                #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
                pub enum Req {
                    /// set which events client is subscribed to
                    #[n(0)]
                    Subscribe(#[n(0)] <super::subscribe::Method as rpc::Method>::Req),
                    /// send a message
                    #[n(1)]
                    Post(#[n(0)] <post::Method as rpc::Method>::Req),
                }

                #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
                pub enum Res {
                    #[n(0)]
                    Subscribe(#[n(0)] <super::subscribe::Method as rpc::Method>::Res),
                    #[n(1)]
                    Post(#[n(0)] <post::Method as rpc::Method>::Res),
                }

                pub struct Method;

                impl rpc::Method for Method {
                    type Req = Req;

                    type Res = Res;

                    type CanTransition = can_transition::False;

                    type IsLeaf = is_leaf::False;
                }

                impl Ancestor<subscribe::Method> for Method {}
                impl Ancestor<post::Method> for Method {}

                impl FromDescendant<subscribe::Method> for Method {
                    fn from_descendant_req(
                        request: <subscribe::Method as rpc::Method>::Req,
                    ) -> <Self as rpc::Method>::Req {
                        Req::Subscribe(request)
                    }

                    fn from_descendant_res(
                        result: <subscribe::Method as rpc::Method>::Res,
                    ) -> <Self as rpc::Method>::Res {
                        Res::Subscribe(result)
                    }

                    fn try_into_descendant_req(
                        request: Self::Req,
                    ) -> Result<<subscribe::Method as rpc::Method>::Req, Self::Req>
                    {
                        match request {
                            Req::Subscribe(child) => Ok(child),
                            other => Err(other),
                        }
                    }
                }

                impl FromDescendant<post::Method> for Method {
                    fn from_descendant_req(
                        request: <post::Method as rpc::Method>::Req,
                    ) -> <Self as rpc::Method>::Req {
                        Req::Post(request)
                    }

                    fn from_descendant_res(
                        result: <post::Method as rpc::Method>::Res,
                    ) -> <Self as rpc::Method>::Res {
                        Res::Post(result)
                    }

                    fn try_into_descendant_req(
                        request: Self::Req,
                    ) -> Result<<post::Method as rpc::Method>::Req, Self::Req> {
                        match request {
                            Req::Post(child) => Ok(child),
                            other => Err(other),
                        }
                    }
                }
            }
            pub mod leave {
                use rpc::{
                    method::{can_transition, is_leaf},
                    state,
                };

                use crate::states::Entrypoint;

                pub struct Method;
                impl rpc::Method for Method {
                    type Req = ();

                    type Res = state::Wrapper<Entrypoint>;

                    type CanTransition = can_transition::True;

                    type IsLeaf = is_leaf::True;
                }
            }
            #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
            pub enum Req {
                /// set which events client is subscribed to
                #[n(0)]
                Loopback(#[n(0)] <loopback::Method as rpc::Method>::Req),
                /// close room
                #[n(1)]
                Close(#[n(0)] <close::Method as rpc::Method>::Req),
                // leave room
                #[n(2)]
                Leave,
            }

            pub enum Res {
                Loopback(<loopback::Method as rpc::Method>::Res),
                Close(state::Wrapper<crate::states::Entrypoint>),
                Leave(state::Wrapper<crate::states::Entrypoint>),
            }

            impl state::Has<crate::states::Entrypoint> for Res {
                fn extract_wrapper(self) -> state::Wrapper<crate::states::Entrypoint> {
                    match self {
                        Res::Loopback(_) => unreachable!(),
                        Res::Close(wrapper) => wrapper,
                        Res::Leave(wrapper) => wrapper,
                    }
                }
            }

            pub struct Method;

            impl rpc::Method for Method {
                type Req = Req;

                type Res = Res;

                type CanTransition = can_transition::True;

                type IsLeaf = is_leaf::False;
            }

            impl Ancestor<subscribe::Method> for Method {}
            impl Ancestor<post::Method> for Method {}
            impl Ancestor<loopback::Method> for Method {}
            impl Ancestor<close::Method> for Method {}
            impl Ancestor<leave::Method> for Method {}

            impl FromDescendant<loopback::Method> for Method {
                fn from_descendant_req(
                    request: <loopback::Method as rpc::Method>::Req,
                ) -> <Self as rpc::Method>::Req {
                    Req::Loopback(request)
                }

                fn from_descendant_res(
                    result: <loopback::Method as rpc::Method>::Res,
                ) -> <Self as rpc::Method>::Res {
                    Res::Loopback(result)
                }

                fn try_into_descendant_req(
                    request: Self::Req,
                ) -> Result<<loopback::Method as rpc::Method>::Req, Self::Req> {
                    match request {
                        Req::Loopback(subscribe_flags) => Ok(subscribe_flags),
                        other => Err(other),
                    }
                }
            }

            impl FromDescendant<leave::Method> for Method {
                fn from_descendant_req(
                    _request: <leave::Method as rpc::Method>::Req,
                ) -> <Self as rpc::Method>::Req {
                    Req::Leave
                }

                fn from_descendant_res(
                    result: <leave::Method as rpc::Method>::Res,
                ) -> <Self as rpc::Method>::Res {
                    Res::Leave(result)
                }

                fn try_into_descendant_req(
                    request: Self::Req,
                ) -> Result<<leave::Method as rpc::Method>::Req, Self::Req> {
                    match request {
                        Req::Leave => Ok(()),
                        other => Err(other),
                    }
                }
            }

            impl FromDescendant<close::Method> for Method {
                fn from_descendant_req(
                    request: <close::Method as rpc::Method>::Req,
                ) -> <Self as rpc::Method>::Req {
                    Req::Close(request)
                }

                fn from_descendant_res(
                    result: <close::Method as rpc::Method>::Res,
                ) -> <Self as rpc::Method>::Res {
                    Res::Close(result)
                }

                fn try_into_descendant_req(
                    request: Self::Req,
                ) -> Result<<close::Method as rpc::Method>::Req, Self::Req> {
                    match request {
                        Req::Close(child) => Ok(child),
                        other => Err(other),
                    }
                }
            }

            impl FromDescendant<from_client::post::Method> for Method {
                fn from_descendant_req(
                    request: <from_client::post::Method as rpc::Method>::Req,
                ) -> <Self as rpc::Method>::Req {
                    Req::Loopback(loopback::Req::Post(request))
                }

                fn from_descendant_res(
                    result: <from_client::post::Method as rpc::Method>::Res,
                ) -> <Self as rpc::Method>::Res {
                    Res::Loopback(loopback::Res::Post(result))
                }

                fn try_into_descendant_req(
                    request: Self::Req,
                ) -> Result<<from_client::post::Method as rpc::Method>::Req, Self::Req>
                {
                    match request {
                        Req::Loopback(loopback::Req::Post(child)) => Ok(child),
                        other => Err(other),
                    }
                }
            }

            impl FromDescendant<subscribe::Method> for Method {
                fn from_descendant_req(
                    request: <subscribe::Method as rpc::Method>::Req,
                ) -> <Self as rpc::Method>::Req {
                    Req::Loopback(loopback::Req::Subscribe(request))
                }

                fn from_descendant_res(
                    result: <subscribe::Method as rpc::Method>::Res,
                ) -> <Self as rpc::Method>::Res {
                    Res::Loopback(loopback::Res::Subscribe(result))
                }

                fn try_into_descendant_req(
                    request: Self::Req,
                ) -> Result<<subscribe::Method as rpc::Method>::Req, Self::Req> {
                    match request {
                        Req::Loopback(loopback::Req::Subscribe(child)) => Ok(child),
                        other => Err(other),
                    }
                }
            }
        }

        pub struct InRoom;

        impl rpc::State for InRoom {
            type ClientHandles = ToClient;

            type ServerHandles = from_client::Method;
        }

        impl Prioritized for InRoom {
            type Priority = u8;

            fn client_priority(
                request: &<Self::ClientHandles as rpc::Method>::Req,
            ) -> Self::Priority {
                use ToClientReq::*;
                match request {
                    Notif(_) => 0,
                    Close(_) => 2,
                }
            }

            fn server_priority(
                request: &<Self::ServerHandles as rpc::Method>::Req,
            ) -> Self::Priority {
                use from_client::Req::*;
                match request {
                    Loopback(_) => 0,
                    Close(_) => 1,
                    Leave => 1,
                }
            }
        }
    }
}

mod server {
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
    use tracing::{debug, trace, warn};

    use crate::{
        RoomId, Username,
        server::room::{AddUserPermit, Room},
        states::{
            Entrypoint,
            entrypoint::join_room::{self},
            in_room::{
                self, close,
                from_client::{self, leave, loopback, post, subscribe},
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
        subscriptions: subscribe::Flags,
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
                subscriptions: subscribe::Flags::empty(),
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
        use std::{collections::HashMap, convert::Infallible, sync::Arc};

        use arrayvec::ArrayVec;
        use futures::{StreamExt as _, TryStreamExt, stream::FuturesUnordered};
        use rpc::traits::HandlerResult;
        use tracing::trace;

        use crate::{
            RoomId, Username,
            server::User,
            states::{
                entrypoint::join_room,
                in_room::{
                    Notification, Notify,
                    from_client::{self, subscribe},
                },
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

            pub fn set_user_subscriptions(&mut self, username: &Username, flags: subscribe::Flags) {
                let Some(user) = self.users.get_mut(username) else {
                    return;
                };
                user.subscriptions = flags;
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
                            if user.subscriptions.check(notification.flag()) {
                                trace!("notifying {} {:?}", user.name, notification);
                                set.push(
                                    user.requester
                                        .request_loopback::<Notify>(notification.clone()),
                                );
                            };
                        }

                        set.try_collect::<()>().await.map_err(|_e| &user.name)?;
                        trace!("finished notifying");
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
        }

        impl<RootMethod: rpc::method::Ancestor<from_client::post::Method>>
            rpc::Handler<RootMethod, from_client::post::Method> for Room
        {
            type Error = Infallible;

            async fn handle<Replier: rpc::ReplyHelper<RootMethod, from_client::post::Method>>(
                &mut self,
                replier: Replier,
                value: rpc::ReqOf<from_client::post::Method>,
            ) -> HandlerResult<RootMethod, from_client::post::Method, Replier, Self::Error>
            {
                self.notify([Notification::Mesg(value)]).await;
                replier.reply(()).await
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

    impl FromClientLoopbackHandler {
        pub fn set_subscription<'a, 'b: 'a>(
            &self,
            room_lock: &mut Room,
            subscriptions: subscribe::Flags,
        ) {
            debug!(
                "setting {} subscriptions: {:?}",
                self.username, subscriptions
            );
            room_lock.set_user_subscriptions(&self.username, subscriptions);
            debug!("set {} subscriptions", self.username);
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

    impl<RootMethod: Ancestor<subscribe::Method>> rpc::Handler<RootMethod, subscribe::Method>
        for FromClientLoopbackHandler
    {
        type Error = Error;

        async fn handle<Replier: rpc::ReplyHelper<RootMethod, subscribe::Method>>(
            &mut self,
            replier: Replier,
            value: rpc::ReqOf<subscribe::Method>,
        ) -> HandlerResult<RootMethod, subscribe::Method, Replier, Self::Error> {
            {
                let mut room_lock = self.room.lock().await;
                let room = room_lock.as_mut().ok_or(Error::RoomClosed)?;
                {
                    self.set_subscription(room, value);
                }
            }
            replier.reply(()).await
        }
    }

    pub trait LoopbackAncestors:
        ancestors::Three<loopback::Method, subscribe::Method, post::Method>
    {
    }

    impl<T: ancestors::Three<loopback::Method, subscribe::Method, post::Method>> LoopbackAncestors
        for T
    {
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
                    loopback::Req::Subscribe(flags) => {
                        replier
                            .reply_with::<subscribe::Method, _>(self, flags, |v| {
                                loopback::Res::Subscribe(v)
                            })
                            .await
                    }
                    loopback::Req::Post(post) => {
                        let mut room_lock = self.room.lock().await;
                        let room = room_lock.as_mut().ok_or(Error::RoomClosed)?;
                        replier
                            .reply_with::<post::Method, _>(room, post, loopback::Res::Post)
                            .await
                            .map_err(|e| match e {
                                rpc::traits::HandleError::Replier(r) => {
                                    rpc::traits::HandleError::Replier(r)
                                }
                                rpc::traits::HandleError::Handler(infallible) => {
                                    match infallible {}
                                }
                            })
                    }
                }
            }
            .await;
            FromClientHandler::block_if_room_closed(result).await
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
                            .reply_with::<close::Method, _>(self, req, |v| {
                                from_client::Res::Close(v)
                            })
                            .await,
                    )
                    .await
                }
                from_client::Req::Leave => {
                    Self::block_if_room_closed(
                        replier
                            .reply_with::<leave::Method, _>(self, (), |v| {
                                from_client::Res::Leave(v)
                            })
                            .await,
                    )
                    .await
                }
            }
        }
    }

    pub async fn handle_incoming(
        incoming: in_memory_transport::Incoming<&'static str>,
        rooms: Rooms,
    ) -> anyhow::Result<()> {
        let conn = incoming.accept().await?;
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
            machine_cursor::processor::MultipleRequestsError<
                std::io::Error,
                Infallible,
                Infallible,
            >,
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
            if let Err(err) = handle_incoming(incoming, rooms.clone()).await {
                warn!("error while handling client: {err}");
            }
        }
    }
}

mod client {
    use std::convert::Infallible;

    use rpc::{
        ReqOf, Transport, in_memory_transport,
        machine_cursor::MachineCursorClient,
        method::{Ancestor, not_applicable},
    };
    use tracing::debug;

    use crate::{
        Username,
        states::{
            entrypoint::join_room,
            in_room::{self, InRoom, ToClient, ToClientRes},
        },
    };

    #[derive(Clone)]
    pub struct ClientHandler {
        username: Username,
    }

    impl ClientHandler {
        pub fn new(username: Username) -> Self {
            Self { username }
        }
    }

    impl<RM: Ancestor<in_room::Notify>> rpc::Handler<RM, in_room::Notify> for ClientHandler {
        type Error = Infallible;

        async fn handle<Replier: rpc::ReplyHelper<RM, in_room::Notify>>(
            &mut self,
            replier: Replier,
            value: ReqOf<in_room::Notify>,
        ) -> rpc::traits::HandlerResult<RM, in_room::Notify, Replier, Self::Error> {
            debug!("{} handling notification", self.username);
            match value {
                in_room::Notification::Join(joiner) => {
                    println!("{}: {joiner} joined", self.username)
                }
                in_room::Notification::Left(leaver) => println!("{}: {leaver} left", self.username),
                in_room::Notification::Mesg(message) => println!("{}: {message}", self.username),
            }
            replier.reply(()).await
        }
    }

    impl<RM: Ancestor<in_room::close::Method>> rpc::Handler<RM, in_room::close::Method>
        for ClientHandler
    {
        type Error = Infallible;

        async fn handle<Replier: rpc::ReplyHelper<RM, in_room::close::Method>>(
            &mut self,
            replier: Replier,
            (): ReqOf<in_room::close::Method>,
        ) -> rpc::traits::HandlerResult<RM, in_room::close::Method, Replier, Self::Error> {
            let wrapper = replier.new_wrapper();
            replier.reply(wrapper).await
        }
    }

    impl<RM: Ancestor<ToClient> + Ancestor<in_room::Notify> + Ancestor<in_room::close::Method>>
        rpc::Handler<RM, ToClient> for ClientHandler
    {
        type Error = Infallible;

        async fn handle<Replier: rpc::ReplyHelper<RM, ToClient>>(
            &mut self,
            replier: Replier,
            value: ReqOf<ToClient>,
        ) -> rpc::traits::HandlerResult<RM, ToClient, Replier, Self::Error> {
            match value {
                in_room::ToClientReq::Notif(notif) => {
                    replier
                        .reply_with::<in_room::Notify, _>(self, notif, |v| {
                            in_room::ToClientRes::Notif(v)
                        })
                        .await
                }
                in_room::ToClientReq::Close(close_req) => {
                    replier
                        .reply_with::<in_room::close::Method, _>(self, close_req, |v| {
                            ToClientRes::Close(v)
                        })
                        .await
                }
            }
        }
    }

    pub async fn join_room(
        username: &'static str,
        network: in_memory_transport::Network<&'static str>,
    ) -> anyhow::Result<MachineCursorClient<InRoom, in_memory_transport::Connection<&'static str>>>
    {
        let transport = network.new_transport(username);
        let cursor = MachineCursorClient::<crate::states::Entrypoint, _>::new(
            transport.connect(&"server").await.unwrap(),
        );
        let mut handler = not_applicable::Handler;
        let (processor, requester) = cursor.into_children_with_handler(&mut handler);
        let (res, requester_transition) = requester
            .request_transition::<join_room::JoinRoom>(join_room::Req {
                room_id: "room_id".try_into().unwrap(),
                username: username.try_into().unwrap(),
            })
            .next()
            .await?
            .try_into_need_processor()
            .ok()
            .unwrap()
            .extract_res();
        let res = res?;
        println!(
            "{} joined; existing participants: {:?}",
            username,
            res.0.inner()
        );
        assert!(res.0.inner().is_empty());
        let in_room = requester_transition.finish(processor, res.1).await?;
        Ok(in_room)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let network = Network::new();

    let server = network.new_transport("server");

    let server_jh = tokio::spawn(server::server(server).instrument(info_span!("server")));

    let span = info_span!("client1");
    let client1_fut = async move {
        let client1_in_room = client::join_room("client1", network).await?;
        debug!("logged in");
        let mut client1_handler = ClientHandler::new("client1".try_into().unwrap());
        let cloned_handler = client1_handler.clone();
        let (processor, requester) =
            client1_in_room.into_children_with_handler(&mut client1_handler);

        let jh = tokio::spawn(
            async move {
                use subscribe::Flags;
                debug!("subscribing");
                requester
                    .request_loopback::<subscribe::Method>(Flags::all())
                    .await?;
                debug!("subscribed");

                requester
                    .request_loopback::<post::Method>(Message {
                        from: "client1".try_into().unwrap(),
                        body: "Hello, World!".try_into().unwrap(),
                    })
                    .await?;
                debug!("sent hello world");

                let transition_request = requester.request_transition::<leave::Method>(());

                anyhow::Ok(transition_request)
            }
            .instrument(tracing::Span::current()),
        );
        let processor_transition = processor.handle_requests::<in_room::Notify, _>(cloned_handler);
        let selection = select(jh, processor_transition).await;
        let tiebreak_result = match selection {
            futures::future::Either::Left((
                requester_transition,
                potential_processor_transition,
            )) => {
                tiebreak::between_potential_processor_and_known_requester_transition(
                    potential_processor_transition,
                    requester_transition.unwrap()?,
                )
                .await?
            }
            futures::future::Either::Right((processor_transition, requester_transition)) => {
                tiebreak::between_processor_and_requester_transition(
                    processor_transition?,
                    requester_transition.await.unwrap()?,
                )
                .await
                .unwrap()
            }
        };

        let _entrypoint_cursor: MachineCursorClient<Entrypoint, _> = match tiebreak_result {
            tiebreak::TiebreakResult::ProcessorWon(finalize_processor_transition) => {
                let (res, finalize_processor_transition) =
                    finalize_processor_transition.extract_res();
                match res {
                    in_room::ToClientRes::Notif(_) => unreachable!(),
                    in_room::ToClientRes::Close(res) => {
                        debug!("close response received");
                        finalize_processor_transition.finish(res).await?
                    }
                }
            }
            tiebreak::TiebreakResult::RequesterWon(finalize_requester_transition) => {
                let (res, finalize_requester_transition) =
                    finalize_requester_transition.extract_res();
                finalize_requester_transition.finish(res).await?
            }
        };
        anyhow::Ok(())
    }
    .instrument(span);

    client1_fut.await?;

    server_jh.abort();

    anyhow::Ok(())
}

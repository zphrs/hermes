use std::{collections::HashMap, sync::Arc};

use arrayvec::ArrayVec;
use futures::{StreamExt as _, TryStreamExt, stream::FuturesUnordered};

use tracing::trace;

use crate::{
    RoomId, Username,
    server::user::{self, User},
    states::{
        entrypoint::join_room,
        in_room::{Notification, Notify},
    },
};

pub struct Room {
    pub id: RoomId,
    users: HashMap<Username, user::User>,
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

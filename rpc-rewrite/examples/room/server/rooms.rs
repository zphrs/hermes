//! Shared room state. Everything here is synchronous (a `std` mutex that is
//! never held across an await) and delivers events to connections through
//! unbounded channels, so it is safe to call from cancel-safe transition
//! leaf handlers and from `Drop`.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::super::{
    RoomId, Username,
    max_len_str::MaxLenStr,
    states::{
        entrypoint::join_room,
        in_room::{Message, Notification},
    },
};

/// Maximum number of users in one room.
pub const MAX_USERS: usize = 10;

/// What the room asks a connection's requester task to do.
#[derive(Debug)]
pub enum Event {
    /// send this notification to the client and await its ack
    Notify(Notification),
    /// the room was closed: transition the client back to `Entrypoint`
    Kick,
}

struct User {
    id: u64,
    tx: UnboundedSender<Event>,
}

#[derive(Default)]
struct Room {
    users: HashMap<Username, User>,
}

impl Room {
    fn notify(&self, notification: &Notification, except: Option<&Username>) {
        for (name, user) in &self.users {
            if Some(name) != except {
                // a closed channel means the connection is already gone
                let _ = user.tx.send(Event::Notify(notification.clone()));
            }
        }
    }
}

#[derive(Default)]
struct Inner {
    rooms: Mutex<HashMap<RoomId, Room>>,
    next_user_id: AtomicU64,
}

/// All rooms of a server. Cheap to clone.
#[derive(Clone, Default)]
pub struct Rooms(Arc<Inner>);

impl Rooms {
    /// Adds `username` to `room_id` (creating the room if needed) and tells
    /// the existing users about it.
    pub fn join(
        &self,
        room_id: RoomId,
        username: Username,
    ) -> Result<(Vec<Username>, Membership), join_room::Error> {
        let mut rooms = self.0.rooms.lock().unwrap();
        let room = rooms.entry(room_id.clone()).or_default();
        // Differs from the old example, which allowed an 11th user (and
        // would have overflowed its own `ArrayVec<_, 10>`): at most
        // `MAX_USERS` users.
        let error = if room.users.len() >= MAX_USERS {
            Some(join_room::Error::RoomFull)
        } else if room.users.contains_key(&username) {
            Some(join_room::Error::UsernameTaken)
        } else {
            None
        };
        if let Some(error) = error {
            if room.users.is_empty() {
                rooms.remove(&room_id);
            }
            return Err(error);
        }
        let existing = room.users.keys().cloned().collect();
        room.notify(&Notification::Join(username.clone()), None);
        let id = self.0.next_user_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = unbounded_channel();
        room.users.insert(username.clone(), User { id, tx });
        let handle = Handle {
            rooms: self.clone(),
            room_id,
            username,
            id,
        };
        Ok((existing, Membership { handle, rx }))
    }
}

/// Identifies one user's membership of one room. Cheap to clone; all
/// operations are idempotent and become no-ops once the user is no longer in
/// the room (removed, room closed, or the name re-used by someone else).
#[derive(Clone)]
pub struct Handle {
    rooms: Rooms,
    room_id: RoomId,
    username: Username,
    id: u64,
}

impl Handle {
    /// Sends `body` to everyone in the room, including the poster.
    pub fn post(&self, body: MaxLenStr<1024>) {
        let rooms = self.rooms.0.rooms.lock().unwrap();
        if let Some(room) = rooms.get(&self.room_id).filter(|r| self.is_member(r)) {
            room.notify(
                &Notification::Mesg(Message {
                    from: self.username.clone(),
                    body,
                }),
                None,
            );
        }
    }

    /// Removes the user; the remaining users get `Left`. An emptied room is
    /// dropped.
    pub fn remove(&self) {
        let mut rooms = self.rooms.0.rooms.lock().unwrap();
        let Some(room) = rooms.get_mut(&self.room_id) else {
            return;
        };
        if !self.is_member(room) {
            return;
        }
        room.users.remove(&self.username);
        if room.users.is_empty() {
            rooms.remove(&self.room_id);
        } else {
            room.notify(&Notification::Left(self.username.clone()), None);
        }
    }

    /// Closes the room: removes it and kicks every other user (the caller is
    /// the one asking, so its own transition is the reply to its request).
    pub fn close_room(&self) {
        let mut rooms = self.rooms.0.rooms.lock().unwrap();
        if !rooms.get(&self.room_id).is_some_and(|r| self.is_member(r)) {
            return;
        }
        let room = rooms.remove(&self.room_id).unwrap();
        for (name, user) in room.users {
            if name != self.username {
                let _ = user.tx.send(Event::Kick);
            }
        }
    }

    fn is_member(&self, room: &Room) -> bool {
        room.users
            .get(&self.username)
            .is_some_and(|u| u.id == self.id)
    }
}

/// RAII membership: removes the user from the room when dropped (leave,
/// connection error, task cancellation), so every exit path is covered.
pub struct Membership {
    pub handle: Handle,
    /// events for this user's connection
    pub rx: UnboundedReceiver<Event>,
}

impl Drop for Membership {
    fn drop(&mut self) {
        self.handle.remove();
    }
}

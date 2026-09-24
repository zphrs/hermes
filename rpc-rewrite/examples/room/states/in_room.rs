use super::super::{Username, max_len_str::MaxLenStr};
use rpc_rewrite::{cursor::state, marker::CanTransition, method};

pub mod close;
pub mod from_client;
pub mod to_client;

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct Message {
    #[n(0)]
    pub from: Username,
    #[n(1)]
    pub body: MaxLenStr<1024>,
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "from {}: {}", self.from, self.body)
    }
}

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum Notification {
    #[n(0)]
    Join(#[n(0)] Username),
    #[n(1)]
    Left(#[n(0)] Username),
    #[n(2)]
    Mesg(#[n(0)] Message),
}

/// Server -> client request: a room event. Normal loopback request whose
/// ack (`()`) is awaited by the server.
pub struct Notify;

impl method::Method for Notify {
    type Req<'buf> = Notification;
    type Res<'buf> = ();

    type Type = method::LeafLoopback;
}

/// Tiebreak: no priority mechanism in rpc-rewrite. (Old: Close 2 > Notify 0
/// on client; Close/Leave 1 > Loopback 0 on server.)
pub struct InRoom;

impl state::State for InRoom {
    type ClientBranchType = CanTransition;
    type ClientHandles = to_client::ToClient;

    type ServerBranchType = CanTransition;
    type ServerHandles = from_client::Method;
}

use super::super::super::{RoomId, Username};
use super::{super::in_room::InRoom, Entrypoint};
use rpc_rewrite::{cursor::state, method};

pub struct JoinRoom;

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct Req {
    #[n(0)]
    pub room_id: RoomId,
    #[n(1)]
    pub username: Username,
}

#[derive(Debug, thiserror::Error, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum Error {
    #[n(0)]
    #[error("room full")]
    RoomFull,
    #[n(1)]
    #[error("username taken")]
    UsernameTaken,
}

/// Up to 10 usernames (enforced by the server, not the type).
pub type Users = Vec<Username>;

/// A refusal carries the [`Wrapper<Entrypoint>`](state::Wrapper) to stay in
/// `Entrypoint`, minted from the same `WrapperCredit` as the success case.
pub type Res = Result<(Users, state::Wrapper<InRoom>), (Error, state::Wrapper<Entrypoint>)>;

impl method::Method for JoinRoom {
    type Req<'buf> = Req;
    type Res<'buf> = Res;

    type Type = method::LeafTransition;
}

impl state::Has<InRoom> for Res {
    fn try_extract_wrapper(self) -> Result<state::Wrapper<InRoom>, Self> {
        match self {
            Ok((_, wrapper)) => Ok(wrapper),
            Err(_) => Err(self),
        }
    }
}

impl state::Has<Entrypoint> for Res {
    fn try_extract_wrapper(self) -> Result<state::Wrapper<Entrypoint>, Self> {
        match self {
            Err((_, wrapper)) => Ok(wrapper),
            Ok(_) => Err(self),
        }
    }
}

use rpc::{
    method::{can_transition, is_leaf},
    state,
};

use crate::{RoomId, Username, states::in_room::InRoom};

use max_sized_vec::MaxSizedVec;

pub struct JoinRoom;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen, Clone)]
pub struct Req {
    #[n(0)]
    pub room_id: RoomId,
    #[n(1)]
    pub username: Username,
}

#[derive(
    Debug, thiserror::Error, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen,
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
    fn try_extract_wrapper(self) -> Result<state::Wrapper<InRoom>, Self> {
        match self {
            Ok(v) => Ok(v.1),
            Err(_) => Err(self),
        }
    }
}

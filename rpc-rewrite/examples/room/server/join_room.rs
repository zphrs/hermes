use super::super::states::{Entrypoint, InRoom, entrypoint::join_room::JoinRoom};
use rpc_rewrite::{
    cursor::state::{Wrapper, WrapperCredit},
    method::{ReqOf, ResOf, TransitionLeafHandler},
};

use super::rooms::{Membership, Rooms};

/// Server handler for `JoinRoom` at `Entrypoint`.
///
/// Has no `await` points, so it cannot be cancelled half way. The room is
/// mutated here (rather than after the transition) because `Entrypoint` is
/// handled with the non-racing `handle_transition_request`: nothing can
/// win against it, and if the reply is lost the returned [`Membership`]
/// (carried as the next handler) removes the user again when dropped.
pub struct JoinHandler {
    pub rooms: Rooms,
}

impl TransitionLeafHandler<JoinRoom> for JoinHandler {
    /// `Some` iff the join succeeded.
    type NextHandler = Option<Membership>;

    async fn handle_transition<'a>(
        self,
        req: ReqOf<'a, JoinRoom>,
        wrapper_credit: WrapperCredit<JoinRoom>,
    ) -> (ResOf<'a, JoinRoom>, Self::NextHandler) {
        match self.rooms.join(req.room_id, req.username) {
            Ok((users, membership)) => {
                let wrapper: Wrapper<InRoom> = wrapper_credit.into();
                (Ok((users, wrapper)), Some(membership))
            }
            Err(error) => {
                let back: Wrapper<Entrypoint> = wrapper_credit.into();
                (Err((error, back)), None)
            }
        }
    }
}

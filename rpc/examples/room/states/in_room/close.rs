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

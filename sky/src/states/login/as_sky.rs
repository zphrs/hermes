use rpc::{
    method::{can_transition, is_leaf},
    state,
};
use shared_schema::SkyNode;

pub type Req = SkyNode;
pub type Res = state::Wrapper<crate::states::sky::State>;

pub struct Method;

impl rpc::Method for Method {
    type Req = Req;

    type Res = Res;

    type CanTransition = can_transition::True;

    type IsLeaf = is_leaf::True;
}

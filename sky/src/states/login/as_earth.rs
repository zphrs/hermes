use rpc::{
    method::{can_transition, is_leaf},
    state,
};
use shared_schema::EarthNode;

pub struct Req {
    pub this: EarthNode,
}
pub struct Res {
    wrapper: state::Wrapper<authenticate::State>,
    challenge: [u8; 32],
}

pub struct Method;

impl rpc::Method for Method {
    type Req = Req;

    type Res = Res;

    type CanTransition = can_transition::True;

    type IsLeaf = is_leaf::True;
}

pub mod authenticate;

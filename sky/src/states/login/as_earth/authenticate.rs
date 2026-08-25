use rpc::{
    method::{can_transition, is_leaf, not_applicable::NotApplicable},
    state,
};

pub struct State;
impl rpc::State for State {
    type ClientHandles = NotApplicable;

    type ServerHandles = Method;
}
pub struct Method;

// TODO: add more fields
pub struct ChallengeResponse;

impl rpc::Method for Method {
    type Req = ChallengeResponse;

    type Res = state::Wrapper<crate::states::earth::State>;

    type CanTransition = can_transition::True;

    type IsLeaf = is_leaf::False;
}

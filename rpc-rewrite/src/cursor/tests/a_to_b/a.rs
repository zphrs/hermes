use crate::cursor::state;
use crate::marker::{Loopback, NotApplicable, Transition, not_applicable};
use crate::method::{
    self, ReqOf, ResOf,
    handler::{TransitionLeafHandler, root_method::RootMethod},
};

pub struct Method;
impl method::Method for Method {
    type Req<'buf> = ();
    type Res<'buf> = state::Wrapper<super::b::State>;

    type Type = method::LeafTransition;
}

impl TransitionLeafHandler for Method {
    type NextHandler = not_applicable::Handler;

    async fn handle_transition<'a>(
        self,
        (): ReqOf<'a, Self>,
        wrapper_credit: state::WrapperCredit<Self>,
    ) -> (ResOf<'a, Self>, Self::NextHandler) {
        (
            state::Wrapper::from_wrapper_credit(wrapper_credit),
            not_applicable::Handler,
        )
    }
}

pub struct State;

impl state::State for State {
    type ClientBranchType = Loopback;
    type ClientHandles = NotApplicable;

    type ServerBranchType = Transition;
    type ServerHandles = RootMethod<Method, Transition>;
}

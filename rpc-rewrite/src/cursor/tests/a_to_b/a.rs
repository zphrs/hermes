use crate::traits::{
    self,
    handler::{TransitionLeafHandler, root_method::RootMethod},
    markers::{False, NotApplicable, True, not_applicable},
    method::{ReqOf, ResOf},
    state,
};

pub struct Method;
impl traits::Method for Method {
    type Req<'buf> = ();

    type Res<'buf> = state::Wrapper<super::b::State>;

    type Transitions = True;

    type HasDescendants = False;
}

impl TransitionLeafHandler for Method {
    type NextHandler = not_applicable::Handler;

    async fn handle_transition<'a>(
        &mut self,
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

impl traits::State for State {
    type ClientHandles = NotApplicable;

    type ServerHandles = RootMethod<Method>;
}

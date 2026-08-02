pub trait HasStateWrapper {
    type State: crate::State;

    fn extract_wrapper(self) -> crate::state::Wrapper<Self::State>;
}

impl<State: crate::State> HasStateWrapper for crate::state::Wrapper<State> {
    type State = State;

    fn extract_wrapper(self) -> crate::state::Wrapper<Self::State> {
        self
    }
}

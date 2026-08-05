pub trait Has<State: crate::State> {
    fn extract_wrapper(self) -> crate::state::Wrapper<State>;
}

impl<State: crate::State> Has<State> for crate::state::Wrapper<State> {
    fn extract_wrapper(self) -> crate::state::Wrapper<State> {
        self
    }
}

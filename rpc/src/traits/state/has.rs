pub trait Has<State: crate::State>: Sized {
    fn try_extract_wrapper(self) -> Result<crate::state::Wrapper<State>, Self>;
}

impl<State: crate::State> Has<State> for crate::state::Wrapper<State> {
    fn try_extract_wrapper(self) -> Result<crate::state::Wrapper<State>, Self> {
        Ok(self)
    }
}

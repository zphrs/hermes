pub mod replier;
pub use replier::Replier;

use super::{ReqOf, ResOf};

pub enum Error<Replier, Handler> {
    Replier(Replier),
    Handler(Handler),
}

mod branch_handler {
    use super::replier::Replier;
    use super::{ReqOf, ResOf};

    pub trait BranchHandler<M: super::super::Branch = Self> {
        fn handle<'a, R: Replier<M>>(
            &mut self,
            request: ReqOf<'a, M>,
            replier: R,
        ) -> impl Future<Output = Result<R::Receipt<ResOf<'a, M>>, R::Error>>;
    }
}

pub use branch_handler::BranchHandler;

pub trait LeafHandler<M: super::Leaf + super::Loopback = Self> {
    fn handle<'a>(&mut self, request: ReqOf<'a, M>) -> impl Future<Output = ResOf<'a, M>>;
}

pub mod transition {
    use super::replier::{Replier, transition};
    use crate::cursor::state::WrapperCredit;

    use super::{ReqOf, ResOf};

    pub type HandleTransitionResult<'a, NextHandler, TR, M> =
        Result<(<TR as Replier<M>>::Receipt<ResOf<'a, M>>, NextHandler), <TR as Replier<M>>::Error>;

    pub trait BranchHandler<M: crate::method::Branch + crate::method::Transitions = Self> {
        type NextHandler;

        fn handle_transition<'a, TR: transition::Replier<M>>(
            self,
            request: ReqOf<'a, M>,
            replier: TR,
        ) -> impl Future<Output = HandleTransitionResult<'a, Self::NextHandler, TR, M>>;
    }

    pub trait LeafHandler<M: crate::method::Leaf + crate::method::Transitions = Self> {
        type NextHandler;

        fn handle_transition<'a>(
            &mut self,
            request: ReqOf<'a, M>,
            wrapper_credit: WrapperCredit<M>,
        ) -> impl Future<Output = (ResOf<'a, M>, Self::NextHandler)>;
    }
}
pub use transition::BranchHandler as TransitionBranchHandler;
pub use transition::LeafHandler as TransitionLeafHandler;
pub mod root_method;

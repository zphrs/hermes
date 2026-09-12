pub mod replier;
pub use replier::Replier;

use crate::method;
use crate::method::LeafLoopback;

use super::{ReqOf, ResOf};

pub enum Error<Replier, Handler> {
    Replier(Replier),
    Handler(Handler),
}

mod branch_handler {

    use crate::Method;

    use super::replier::Replier;
    use super::{ReqOf, ResOf};

    pub trait BranchHandler<M: Method = Self> {
        fn handle<'a, R: Replier<M>>(
            &mut self,
            request: ReqOf<'a, M>,
            replier: R,
        ) -> impl Future<Output = Result<R::Receipt<ResOf<'a, M>>, R::Error>>;
    }
}

pub use branch_handler::BranchHandler;

pub trait LeafHandler<M: method::OfType<LeafLoopback> = Self> {
    fn handle<'a>(&mut self, request: ReqOf<'a, M>) -> impl Future<Output = ResOf<'a, M>>;
}

pub mod loopback {
    use crate::method::{self, Loopback, ReqOf, ResOf, replier};

    pub trait BranchHandler<M: method::OfType<method::Branch<Loopback>> = Self> {
        fn handle_loopback<'a, R: replier::Replier<M> + replier::loopback::Replier<M>>(
            &mut self,
            request: ReqOf<'a, M>,
            replier: R,
        ) -> impl std::future::Future<
            Output = Result<<R as super::Replier<M>>::Receipt<ResOf<'a, M>>, R::Error>,
        >;
    }
}

pub mod can_transition {
    use crate::method::{self, Replier, ReqOf, ResOf, replier};

    pub type PossibleTransitionResult<'a, R, M, NextHandler> = Result<
        (
            <R as Replier<M>>::Receipt<ResOf<'a, M>>,
            Option<NextHandler>,
        ),
        <R as Replier<M>>::Error,
    >;
    pub trait BranchHandler<M: method::OfType<method::BranchCanTransition> = Self> {
        type NextHandler;

        fn handle_possible_transition<
            'a,
            R: Replier<M> + replier::loopback::Replier<M> + replier::transition::Replier<M>,
        >(
            self,
            request: ReqOf<'a, M>,
            replier: R,
        ) -> impl Future<Output = PossibleTransitionResult<'a, R, M, Self::NextHandler>>;
    }
}

pub use can_transition::BranchHandler as HybridBranchHandler;

pub mod transition {
    use super::replier::transition;
    use crate::{cursor::state::WrapperCredit, method};

    use super::{ReqOf, ResOf};

    pub type HandleTransitionResult<'a, NextHandler, TR, M> = Result<
        (
            <TR as super::Replier<M>>::Receipt<ResOf<'a, M>>,
            NextHandler,
        ),
        <TR as super::replier::Replier<M>>::Error,
    >;

    pub trait BranchHandler<M: method::OfType<method::Branch<method::Transition>> = Self> {
        type NextHandler;

        fn handle_transition<'a, TR: transition::Replier<M>>(
            self,
            request: ReqOf<'a, M>,
            replier: TR,
        ) -> impl Future<Output = HandleTransitionResult<'a, Self::NextHandler, TR, M>>;
    }

    pub trait LeafHandler<M: method::OfType<method::LeafTransition> = Self> {
        type NextHandler;

        fn handle_transition<'a>(
            self,
            request: ReqOf<'a, M>,
            wrapper_credit: WrapperCredit<M>,
        ) -> impl Future<Output = (ResOf<'a, M>, Self::NextHandler)>;
    }
}
pub use transition::BranchHandler as TransitionBranchHandler;
pub use transition::LeafHandler as TransitionLeafHandler;
pub mod root_method;

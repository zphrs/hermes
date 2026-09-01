use crate::traits::method::{self, ReqOf, ResOf};

pub enum Error<Replier, Handler> {
    Replier(Replier),
    Handler(Handler),
}

pub mod replier;

pub use replier::Replier;
mod bh {
    use crate::traits::{
        Replier,
        method::{self, Loopback, ReqOf, ResOf},
    };

    pub trait BranchHandler<M: method::Branch = Self> {
        fn handle<'a, R: Replier<M>>(
            &mut self,
            request: ReqOf<'a, M>,
            replier: R,
        ) -> impl Future<Output = Result<R::Receipt<ResOf<'a, M>>, R::Error>>;
    }
}

pub use bh::BranchHandler;

pub trait LeafHandler<M: method::Leaf + method::Loopback = Self> {
    fn handle<'a>(&mut self, request: ReqOf<'a, M>) -> impl Future<Output = ResOf<'a, M>>;
}

pub mod transition {
    use crate::traits::{
        method::{self, ReqOf, ResOf},
        replier::{Replier, transition},
        state::WrapperCredit,
    };

    pub type HandleTransitionResult<'a, NextHandler, TR, M> =
        Result<(<TR as Replier<M>>::Receipt<ResOf<'a, M>>, NextHandler), <TR as Replier<M>>::Error>;

    pub trait BranchHandler<M: method::Branch + method::Transitions = Self> {
        type NextHandler;

        fn handle_transition<'a, TR: transition::Replier<M>>(
            self,
            request: ReqOf<'a, M>,
            replier: TR,
        ) -> impl Future<Output = HandleTransitionResult<'a, Self::NextHandler, TR, M>>;
    }

    pub trait LeafHandler<M: method::Leaf + method::Transitions = Self> {
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

#[cfg(test)]
mod tests;

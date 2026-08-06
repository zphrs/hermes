pub mod ancestor;
mod from_descendant;

pub use from_descendant::FromDescendant;

pub use ancestor::Ancestor;

use std::marker::PhantomData;

pub mod can_transition {
    //! Marker for a given method to declare whether or not the method might
    //! result in a transition or not. Primarily used by
    //! [`State`](crate::traits::State) and the
    //! [`MachineWalker`](crate::MachineWalker) to determine which
    //! [`Method`](super::Method)s can be passed into various transitioning and
    //! non-transitioning functions.

    /// either [`True`] or [`False`]
    pub(super) trait Transitions {}
    /// indicates that the method does transition
    pub struct True;

    impl Transitions for True {}
    /// indicates that the method does not transition
    pub struct False;

    impl Transitions for False {}
}

pub trait Loopback: Method<CanTransition = can_transition::False> {}

impl<M: Method<CanTransition = can_transition::False>> Loopback for M {}

pub trait CanTransition: Method<CanTransition = can_transition::True> {}

impl<M: Method<CanTransition = can_transition::True>> CanTransition for M {}
pub mod is_leaf {

    pub(super) trait IsLeaf {}

    pub struct False;

    pub struct True;

    impl IsLeaf for True {}
    impl IsLeaf for False {}
}
pub trait Method {
    type Req;
    type Res;

    /// See [can_transition]
    #[expect(
        private_bounds,
        reason = "Transitions is more private to force users to either use
        can_transition::True or can_transition::False as a marker struct"
    )]
    type CanTransition: can_transition::Transitions;
    #[expect(
        private_bounds,
        reason = "IsLeaf is more private to force users to either use
        is_leaf::True or is_leaf::False as a marker struct"
    )]
    // Whether this method does not contain any sub-methods.
    type IsLeaf: is_leaf::IsLeaf;
}

pub mod not_applicable {
    use crate::{method::is_leaf, traits::method::can_transition};
    use std::convert::Infallible;

    /// Type for a method whose requests and responses are impossible to construct;
    /// used to specify no method at all for a one-sided
    /// [`State`](crate::traits::State).
    #[derive(Clone)]
    pub enum NotApplicable {}

    impl PartialEq for NotApplicable {
        fn eq(&self, _other: &Self) -> bool {
            false
        }
    }

    impl PartialOrd for NotApplicable {
        fn partial_cmp(&self, _other: &Self) -> Option<std::cmp::Ordering> {
            None
        }
    }

    type Request = NotApplicable;
    type Response = NotApplicable;

    pub type Method = NotApplicable;

    impl crate::Method for Method {
        type Req = Request;
        type Res = Response;

        type CanTransition = can_transition::False;

        type IsLeaf = is_leaf::True;
    }

    pub struct Handler;

    impl<RootMethod> crate::Handler<RootMethod, Method> for Handler {
        type Error = Infallible;

        async fn handle<Replier: crate::transport::ReplyHelper<Method, RootMethod>>(
            &mut self,
            _replier: Replier,
            _value: <Method as super::Method>::Req,
        ) -> Result<
            <Replier as crate::transport::ReplyHelper<Method, RootMethod>>::Receipt<Method>,
            crate::traits::HandleError<
                <Replier as crate::transport::ReplyHelper<Method, RootMethod>>::Error,
                <Self as crate::Handler<RootMethod, Method>>::Error,
            >,
        > {
            unimplemented!("no point in implementing since the request can't be constructed")
        }
    }
}

pub struct Wrapper<Method: crate::traits::Method> {
    _marker: PhantomData<Method>,
}

impl<Method: crate::traits::Method> Wrapper<Method> {
    pub(crate) fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

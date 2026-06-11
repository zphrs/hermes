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
}
pub mod not_applicable {
    use crate::traits::method::can_transition;
    use std::convert::Infallible;

    /// Type for a method whose requests and responses are impossible to construct;
    /// used to specify no method at all for a one-sided
    /// [`State`](crate::traits::State).
    pub enum NotApplicable {}

    type Request = NotApplicable;
    type Response = NotApplicable;

    pub type Method = NotApplicable;

    impl crate::Method for Method {
        type Req = Request;
        type Res = Response;

        type CanTransition = can_transition::False;
    }

    pub struct Handler;

    impl crate::Handler<Method> for Handler {
        type Error = Infallible;

        async fn handle<Replier: crate::transport::ReplyHelper<NotApplicable>>(
            &mut self,
            _replier: Replier,
            _value: <Method as crate::Method>::Req,
        ) -> Result<
            Replier::Receipt<NotApplicable>,
            crate::traits::HandlerError<Replier::Error, Self::Error>,
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

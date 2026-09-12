mod descendant;

pub use descendant::Descendant;

use crate::marker::{self, MethodType, NotApplicable};

pub use marker::{Branch, CanTransition, Leaf, Loopback, Transition};

pub mod handler;
pub use handler::replier;

pub trait Method {
    type Req<'buf>;
    type Res<'buf>;
    /// whether a request can transition
    #[expect(private_bounds)]
    type Type: MethodType;
}

pub type ReqOf<'buf, M> = <M as Method>::Req<'buf>;
pub type ResOf<'buf, M> = <M as Method>::Res<'buf>;

pub type LeafLoopback = Leaf<Loopback>;

pub type LeafTransition = Leaf<Transition>;

pub type BranchCanTransition = Branch<CanTransition>;

pub trait Notification: for<'a> Method<Res<'a> = NotApplicable> {}

impl<T: for<'a> Method<Res<'a> = NotApplicable> + ?Sized> Notification for T {}

pub use handler::{
    BranchHandler, LeafHandler, Replier, TransitionBranchHandler, TransitionLeafHandler,
};

pub trait OfType<Type>: Method<Type = Type> {}

impl<Type, T: Method<Type = Type> + ?Sized> OfType<Type> for T {}

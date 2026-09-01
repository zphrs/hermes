pub mod can_transition;
mod descendant;
pub mod has_descendants;

pub use descendant::Descendant;

use crate::traits::markers::NotApplicable;

pub mod handler;

pub trait Method {
    type Req<'buf>;
    type Res<'buf>;
    /// whether a request can transition
    #[expect(private_bounds)]
    type Transitions: can_transition::Sealed;
    /// whether a method contains a sub-method
    #[expect(private_bounds)]
    type HasDescendants: has_descendants::Sealed;
}

pub type ReqOf<'buf, M> = <M as Method>::Req<'buf>;
pub type ResOf<'buf, M> = <M as Method>::Res<'buf>;

pub trait Loopback: Method<Transitions = super::markers::False> {}
pub trait Transitions: Method<Transitions = super::markers::True> {}

impl<T: Method<Transitions = super::markers::False> + ?Sized> Loopback for T {}
impl<T: Method<Transitions = super::markers::True> + ?Sized> Transitions for T {}

pub trait Branch: Method<HasDescendants = super::markers::True> {}
pub trait Leaf: Method<HasDescendants = super::markers::False> {}

impl<T: Method<HasDescendants = super::markers::True> + ?Sized> Branch for T {}
impl<T: Method<HasDescendants = super::markers::False> + ?Sized> Leaf for T {}

pub trait Notification: for<'a> Method<Res<'a> = NotApplicable> {}

impl<T: for<'a> Method<Res<'a> = NotApplicable> + ?Sized> Notification for T {}

pub mod can_transition;
mod descendant;
pub mod has_descendants;

pub use descendant::Descendant;

use crate::markers::NotApplicable;

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

pub trait Loopback: Method<Transitions = crate::markers::False> {}
pub trait Transitions: Method<Transitions = crate::markers::True> {}

impl<T: Method<Transitions = crate::markers::False> + ?Sized> Loopback for T {}
impl<T: Method<Transitions = crate::markers::True> + ?Sized> Transitions for T {}

pub trait Branch: Method<HasDescendants = crate::markers::True> {}
pub trait Leaf: Method<HasDescendants = crate::markers::False> {}

impl<T: Method<HasDescendants = crate::markers::True> + ?Sized> Branch for T {}
impl<T: Method<HasDescendants = crate::markers::False> + ?Sized> Leaf for T {}

pub trait Notification: for<'a> Method<Res<'a> = NotApplicable> {}

impl<T: for<'a> Method<Res<'a> = NotApplicable> + ?Sized> Notification for T {}

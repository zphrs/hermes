pub mod can_transition;
mod descendant;
pub mod has_descendants;

pub use descendant::Descendant;

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

pub trait Loopback: Method<Transitions = can_transition::True> {}
pub trait Transitions: Method<Transitions = can_transition::False> {}

impl<T: Method<Transitions = can_transition::True> + ?Sized> Loopback for T {}
impl<T: Method<Transitions = can_transition::False> + ?Sized> Transitions for T {}

pub trait Branch: Method<HasDescendants = has_descendants::True> {}
pub trait Leaf: Method<HasDescendants = has_descendants::False> {}

impl<T: Method<HasDescendants = has_descendants::True> + ?Sized> Branch for T {}
impl<T: Method<HasDescendants = has_descendants::False> + ?Sized> Leaf for T {}

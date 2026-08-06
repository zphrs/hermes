pub(crate) trait Descendant<A: ?Sized>: crate::Method {}
pub trait Ancestor<D: crate::Method + ?Sized>: crate::Method {}

impl<T: crate::Method> Ancestor<T> for T {}

impl<A: ?Sized + Ancestor<D>, D: crate::Method + ?Sized> Descendant<A> for D where A: Ancestor<D> {}

#[expect(private_bounds)]
pub trait Leaf<RootMethod>: Descendant<RootMethod, IsLeaf = super::is_leaf::True> {}

impl<RootMethod: Ancestor<T>, T: crate::Method<IsLeaf = super::is_leaf::True> + ?Sized>
    Leaf<RootMethod> for T
{
}

#[expect(private_bounds)]
pub trait Branch<RootMethod>: Descendant<RootMethod, IsLeaf = super::is_leaf::False> {}

impl<RootMethod: Ancestor<T>, T: crate::Method<IsLeaf = super::is_leaf::False> + ?Sized>
    Branch<RootMethod> for T
{
}

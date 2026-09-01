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

pub mod ancestors {
    macro_rules! define_ancestors {
        ($name:ident; $($t:ident),+) => {
            pub trait $name<$($t: crate::Method),+>: $(super::Ancestor<$t> +)+ {}

            impl<RootMethod, $($t: crate::Method),+> $name<$($t),+> for RootMethod
            where
                RootMethod: $(super::Ancestor<$t> +)+
            {}
        };
    }

    define_ancestors!(One; A);
    define_ancestors!(Two; A, B);
    define_ancestors!(Three; A, B, C);
    define_ancestors!(Four; A, B, C, D);
    define_ancestors!(Five; A, B, C, D, E);
    define_ancestors!(Six; A, B, C, D, E, F);
    define_ancestors!(Seven; A, B, C, D, E, F, G);
    define_ancestors!(Eight; A, B, C, D, E, F, G, H);
    define_ancestors!(Sixteen; A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q);
    define_ancestors!(ThirtyTwo; A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
        AA, AB, AC, AD, AE, AF, AG);
    define_ancestors!(SixtyFour; A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
        AA, AB, AC, AD, AE, AF, AG, AH, AI, AJ, AK, AL, AM, AN, AO, AP, AQ, AR, AS, AT, AU, AV, AW, AX, AY, AZ,
        BA, BB, BC, BD, BE, BF, BG, BH, BI, BJ, BK, BL);
}

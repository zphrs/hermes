use crate::traits::method::{self, ReqOf, ResOf};

pub enum Error<Replier, Handler> {
    Replier(Replier),
    Handler(Handler),
}

pub mod replier;

pub trait BranchHandler<M: method::Branch = Self> {
    fn handle<'a, Replier: replier::Replier<M>>(
        &mut self,
        request: ReqOf<'a, M>,
        replier: Replier,
    ) -> impl Future<Output = Result<Replier::Receipt<'a, ResOf<'a, M>>, Replier::Error>>;
}
pub trait LeafHandler<M: method::Leaf = Self> {
    fn handle<'a>(&mut self, request: ReqOf<'a, M>) -> impl Future<Output = ResOf<'a, M>>;
}

pub mod root_method {
    use std::marker::PhantomData;

    use crate::traits::method::{
        self, Descendant, Method, ReqOf, ResOf,
        handler::{BranchHandler, LeafHandler, replier},
        has_descendants,
    };

    pub struct RootMethod<M: method::Method>(PhantomData<M>);

    impl<M: method::Method> Method for RootMethod<M> {
        type Req<'buf> = ReqOf<'buf, M>;

        type Res<'buf> = ResOf<'buf, M>;

        type Transitions = M::Transitions;

        type HasDescendants = has_descendants::True;
    }

    impl<M: method::Method> Descendant<RootMethod<M>> for M {
        fn req_from_parent<'buf>(req: ReqOf<'buf, RootMethod<M>>) -> ReqOf<'buf, Self> {
            req
        }

        fn res_to_parent<'buf>(res: ResOf<'buf, RootMethod<M>>) -> ResOf<'buf, Self> {
            res
        }
    }

    pub struct RootHandler<Handler>(pub Handler);

    impl<M: method::Leaf, Handler: LeafHandler<M>> BranchHandler<RootMethod<M>> for RootHandler<Handler>
    where
        for<'a> ResOf<'a, M>: minicbor::CborLen<()> + minicbor::Encode<()>,
    {
        async fn handle<'a, Replier: replier::Replier<RootMethod<M>>>(
            &mut self,
            request: ReqOf<'a, RootMethod<M>>,
            replier: Replier,
        ) -> Result<Replier::Receipt<'a, ResOf<'a, RootMethod<M>>>, Replier::Error> {
            replier.reply_with_leaf(request, &mut self.0).await
        }
    }
}

#[cfg(test)]
mod tests;

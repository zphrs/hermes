use crate::traits::method::{self, ReqOf, ResOf};

pub trait Replier<M: method::Branch> {
    type Receipt<'buf, Res>: Receipt<Res>;
    type Error;

    fn reply_with_branch<
        'buf,
        Descendant: method::Branch + method::Descendant<M>,
        DescendantHandler: method::handler::BranchHandler<Descendant>,
    >(
        self,
        request: ReqOf<'buf, Descendant>,
        handler: &mut DescendantHandler,
    ) -> impl Future<Output = Result<Self::Receipt<'buf, ResOf<'buf, M>>, Self::Error>>;

    fn reply_with_leaf<
        'buf,
        Descendant: method::Leaf + method::Descendant<M>,
        DescendantHandler: method::handler::LeafHandler<Descendant>,
    >(
        self,
        request: ReqOf<'buf, Descendant>,
        handler: &mut DescendantHandler,
    ) -> impl Future<Output = Result<Self::Receipt<'buf, ResOf<'buf, M>>, Self::Error>>
    where
        ResOf<'buf, Descendant>: minicbor::Encode<()> + minicbor::CborLen<()>;
}

pub trait Receipt<Res> {
    type Error;
    fn finalize(self) -> impl Future<Output = Result<Res, Self::Error>>;
}

pub mod futures_io;

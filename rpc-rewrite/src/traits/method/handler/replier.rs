use crate::traits::method::{self, ReqOf, ResOf};

pub trait Replier<M: method::Branch> {
    type Receipt<Res>: Receipt<Res>;
    type Error;

    fn reply_with_branch<
        'req,
        Descendant: method::Branch + method::Descendant<M>,
        DescendantHandler: method::handler::BranchHandler<Descendant>,
    >(
        self,
        request: ReqOf<'req, Descendant>,
        handler: &mut DescendantHandler,
    ) -> impl Future<Output = Result<Self::Receipt<ResOf<'req, M>>, Self::Error>>;

    fn reply_with_leaf<
        'req,
        Descendant: method::Leaf + method::Descendant<M> + method::Loopback,
        DescendantHandler: method::handler::LeafHandler<Descendant>,
    >(
        self,
        request: ReqOf<'req, Descendant>,
        handler: &mut DescendantHandler,
    ) -> impl Future<Output = Result<Self::Receipt<ResOf<'req, M>>, Self::Error>>
    where
        ResOf<'req, Descendant>: minicbor::Encode<()> + minicbor::CborLen<()>;
}

pub mod transition {
    use crate::traits::method::{self, ReqOf, ResOf};

    pub trait Replier<M: method::Branch + method::Transitions>: super::Replier<M> {
        fn reply_with_leaf<
            'req,
            Descendant: method::Leaf + method::Descendant<M> + method::Transitions,
            DescendantHandler: method::handler::transition::LeafHandler<Descendant>,
        >(
            self,
            request: ReqOf<'req, Descendant>,
            handler: DescendantHandler,
        ) -> impl Future<
            Output = Result<
                (
                    Self::Receipt<ResOf<'req, M>>,
                    DescendantHandler::NextHandler,
                ),
                Self::Error,
            >,
        >
        where
            ResOf<'req, Descendant>: minicbor::Encode<()> + minicbor::CborLen<()>;
    }
}

pub trait Receipt<Res> {
    type Error;
    fn finalize(self) -> impl Future<Output = Result<Res, Self::Error>>;
}

pub mod futures_io;

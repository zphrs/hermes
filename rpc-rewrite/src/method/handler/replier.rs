use crate::{
    Method,
    marker::BranchType,
    method::{self, ReqOf, ResOf},
};

pub trait Replier<M: Method> {
    type Receipt<Res>: Receipt<Res>;
    type Error;

    fn reply_with_branch<
        'req,
        BT: BranchType,
        Descendant: method::OfType<method::Branch<BT>> + method::Descendant<M>,
        DescendantHandler: method::handler::BranchHandler<Descendant>,
    >(
        self,
        request: ReqOf<'req, Descendant>,
        handler: &mut DescendantHandler,
    ) -> impl Future<Output = Result<Self::Receipt<ResOf<'req, M>>, Self::Error>>;
}

pub mod loopback {
    use crate::{
        Method,
        method::{self, LeafLoopback, ReqOf, ResOf},
    };

    pub trait Replier<M: Method>: super::Replier<M> {
        fn reply_with_leaf<
            'req,
            Descendant: method::OfType<LeafLoopback> + method::Descendant<M>,
            DescendantHandler: method::handler::LeafHandler<Descendant>,
        >(
            self,
            request: ReqOf<'req, Descendant>,
            handler: &mut DescendantHandler,
        ) -> impl Future<Output = Result<Self::Receipt<ResOf<'req, M>>, Self::Error>>
        where
            ResOf<'req, Descendant>: minicbor::Encode<()> + minicbor::CborLen<()>;
    }
}

pub mod transition {

    use crate::{
        Method,
        method::{self, ReqOf, ResOf},
    };

    pub type ReplyResult<'req, R, M, DescendantHandler, Descendant> = Result<
        (
            <R as super::Replier<M>>::Receipt<ResOf<'req, M>>,
            <DescendantHandler as method::handler::transition::LeafHandler<Descendant>>::NextHandler,
        ),
        <R as super::Replier<M>>::Error,
    >;

    pub trait Replier<M: Method>: super::Replier<M> {
        #[allow(clippy::type_complexity)]
        fn transition_with_leaf<
            'req,
            Descendant: method::OfType<method::LeafTransition> + method::Descendant<M>,
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

pub mod immediate;

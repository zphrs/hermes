use std::marker::PhantomData;

use futures::AsyncWrite;
use maxlen::MaxLen;
use minicbor::CborLen as _;

use crate::{
    RpcMessage,
    method::{
        Ancestor,
        ancestor::{Branch, Leaf},
    },
    state::HasStateWrapper,
    traits::{
        self,
        method::{self},
    },
};

/// Returned when you call [`reply`](Call::reply) on a [Method] that implements
/// [Call].
///
/// Used to enforce calling [`reply`](Call::reply) at some point within the
/// [`handle`](RootHandler::handle) function as all possible request types
/// should be replied to.
// Can only construct within the transport module
pub struct ReplyReceipt<M: crate::Method>(method::Wrapper<M>, pub(super) M::Res);

impl<M: crate::Method> ReplyReceipt<M> {
    pub(crate) fn new(t: M::Res) -> Self {
        Self(method::Wrapper::new(), t)
    }

    fn map<NewMethod: crate::Method>(
        self,
        mapper: impl FnOnce(M::Res) -> NewMethod::Res,
    ) -> ReplyReceipt<NewMethod> {
        ReplyReceipt::new(mapper(self.into_inner()))
    }

    pub(crate) fn into_inner(self) -> M::Res {
        self.1
    }
}

pub struct ImmediateReplier<T, Method: ?Sized>
where
    T: AsyncWrite,
{
    client: minicbor_io::AsyncWriter<T>,
    _marker: PhantomData<Method>,
}

impl<T, Method: ?Sized> From<T> for ImmediateReplier<T, Method>
where
    T: AsyncWrite,
{
    fn from(client: T) -> Self {
        Self {
            client: minicbor_io::AsyncWriter::new(client),
            _marker: PhantomData,
        }
    }
}

impl<T, Method: crate::Method> ImmediateReplier<T, Method>
where
    T: AsyncWrite,
{
    pub(crate) fn new(client: minicbor_io::AsyncWriter<T>) -> Self {
        Self {
            client,
            _marker: Default::default(),
        }
    }

    fn change_method<NewMethod: crate::Method>(
        self,
        _req: &NewMethod::Req,
    ) -> ImmediateReplier<T, NewMethod> {
        ImmediateReplier::new(self.client)
    }
}

pub trait ReplyHelper<Method: crate::Method, RootMethod>: Sized {
    type Error;
    type Receipt<M: crate::Method>;
    fn reply<Error>(
        self,
        res: Method::Res,
    ) -> impl Future<
        Output = Result<Self::Receipt<Method>, crate::traits::HandleError<Self::Error, Error>>,
    >
    where
        Method::Res: RpcMessage,
        Method: Leaf<RootMethod>;

    fn reply_with<NewMethod: crate::Method, Handler: traits::Handler<RootMethod, NewMethod>>(
        self,
        handler: &mut Handler,
        req: NewMethod::Req,
        convert: impl FnOnce(NewMethod::Res) -> Method::Res,
    ) -> impl Future<
        Output = Result<
            Self::Receipt<Method>,
            crate::traits::HandleError<Self::Error, Handler::Error>,
        >,
    >
    where
        Method: Branch<RootMethod>,
        RootMethod: Ancestor<NewMethod>;

    fn new_wrapper(&self) -> crate::state::Wrapper<<Method::Res as HasStateWrapper>::State>
    where
        Method::Res: HasStateWrapper,
    {
        crate::state::Wrapper::new_without_check()
    }
}

impl<T, Method: crate::Method, RootMethod> ReplyHelper<Method, RootMethod>
    for ImmediateReplier<T, Method>
where
    T: AsyncWrite + Unpin,
{
    type Error = minicbor_io::Error;
    type Receipt<M: crate::Method> = ReplyReceipt<M>;

    fn reply<Error>(
        mut self,
        res: Method::Res,
    ) -> impl Future<
        Output = Result<Self::Receipt<Method>, crate::traits::HandleError<Self::Error, Error>>,
    >
    where
        Method::Res: RpcMessage,
    {
        async move {
            assert!(res.cbor_len(&mut ()) <= Method::Res::max_len());
            let written = self.client.write(&res).await;
            written
                .map(move |_| ReplyReceipt::new(res))
                .map_err(crate::traits::HandleError::Replier)
        }
    }

    async fn reply_with<
        NewMethod: crate::Method,
        Handler: traits::Handler<RootMethod, NewMethod>,
    >(
        self,
        handler: &mut Handler,
        req: NewMethod::Req,
        convert: impl FnOnce(NewMethod::Res) -> Method::Res,
    ) -> Result<Self::Receipt<Method>, crate::traits::HandleError<Self::Error, Handler::Error>>
    {
        Ok(handler
            .handle(self.change_method(&req), req)
            .await?
            .map(convert))
    }
}

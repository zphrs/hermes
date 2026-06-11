use std::marker::PhantomData;

use futures::AsyncWrite;
use maxlen::MaxLen;
use minicbor::CborLen as _;

use crate::{
    RpcMessage,
    traits::{self, method},
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

pub struct ImmediateReplier<'a, T: AsyncWrite, Method: ?Sized> {
    client: &'a mut minicbor_io::AsyncWriter<T>,
    _marker: PhantomData<Method>,
}

impl<'a, T: AsyncWrite, Method: crate::Method> ImmediateReplier<'a, T, Method> {
    pub(crate) fn new(client: &'a mut minicbor_io::AsyncWriter<T>) -> Self {
        Self {
            client,
            _marker: Default::default(),
        }
    }

    fn change_method<NewMethod: crate::Method>(
        self,
        _req: &NewMethod::Req,
    ) -> ImmediateReplier<'a, T, NewMethod> {
        ImmediateReplier::new(self.client)
    }
}

pub trait ReplyHelper<Method: crate::Method> {
    type Error;
    type Receipt<M: crate::Method>;
    fn reply<Error>(
        self,
        res: Method::Res,
    ) -> impl Future<
        Output = Result<Self::Receipt<Method>, crate::traits::HandlerError<Self::Error, Error>>,
    >
    where
        Method::Res: RpcMessage;

    fn reply_with<NewMethod: crate::Method, Handler: traits::Handler<NewMethod>>(
        self,
        handler: &mut Handler,
        req: NewMethod::Req,
        convert: impl FnOnce(NewMethod::Res) -> Method::Res,
    ) -> impl Future<
        Output = Result<
            Self::Receipt<Method>,
            crate::traits::HandlerError<Self::Error, Handler::Error>,
        >,
    >;
}

impl<'a, T: AsyncWrite + Unpin, Method: crate::Method> ReplyHelper<Method>
    for ImmediateReplier<'a, T, Method>
{
    type Error = minicbor_io::Error;
    type Receipt<M: crate::Method> = ReplyReceipt<M>;

    fn reply<Error>(
        self,
        res: Method::Res,
    ) -> impl Future<
        Output = Result<Self::Receipt<Method>, crate::traits::HandlerError<Self::Error, Error>>,
    >
    where
        Method::Res: RpcMessage,
    {
        async {
            assert!(res.cbor_len(&mut ()) <= Method::Res::max_len());
            let written = self.client.write(&res).await;
            written
                .map(move |_| ReplyReceipt::new(res))
                .map_err(crate::traits::HandlerError::Replier)
        }
    }

    async fn reply_with<NewMethod: crate::Method, Handler: traits::Handler<NewMethod>>(
        self,
        handler: &mut Handler,
        req: NewMethod::Req,
        convert: impl FnOnce(NewMethod::Res) -> Method::Res,
    ) -> Result<Self::Receipt<Method>, crate::traits::HandlerError<Self::Error, Handler::Error>>
    {
        Ok(handler
            .handle(self.change_method(&req), req)
            .await?
            .map(convert))
    }
}

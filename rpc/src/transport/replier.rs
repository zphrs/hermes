use std::marker::PhantomData;

use futures::AsyncWrite;
use maxlen::MaxLen;
use minicbor::CborLen as _;

use crate::RpcMessage;

/// Returned when you call [`reply`](Call::reply) on a [Method] that implements
/// [Call].
///
/// Used to enforce calling [`reply`](Call::reply) at some point within the
/// [`handle`](RootHandler::handle) function as all possible request types
/// should be replied to.
// Can only construct within the transport module
pub struct ReplyReceipt<T = ()>(pub(super) T);

impl<T> ReplyReceipt<T> {
    #[must_use]
    pub fn take(self) -> (T, ReplyReceipt<()>) {
        (self.0, ReplyReceipt(()))
    }
    #[must_use]
    pub(crate) fn into_inner(self) -> T {
        self.0
    }
    #[must_use]
    pub fn map<N>(self, f: impl FnOnce(T) -> N) -> ReplyReceipt<N> {
        ReplyReceipt(f(self.0))
    }
    #[must_use]
    pub fn replace<N>(self, new_inner: N) -> (T, ReplyReceipt<N>) {
        (self.0, ReplyReceipt(new_inner))
    }
    #[must_use]
    pub fn clear(self) -> ReplyReceipt {
        ReplyReceipt(())
    }
}

pub struct Replier<'a, T: AsyncWrite + Unpin + Send + Sync, Method: ?Sized> {
    client: &'a mut minicbor_io::AsyncWriter<T>,
    _marker: PhantomData<Method>,
}

impl<'a, T: AsyncWrite + Unpin + Send + Sync, Method: crate::Method + ?Sized>
    Replier<'a, T, Method>
{
    pub(crate) fn new(client: &'a mut minicbor_io::AsyncWriter<T>) -> Self {
        Self {
            client,
            _marker: Default::default(),
        }
    }
    // take in the request type to verify that the client's request actually
    // contained the method
    pub fn change_method<M: crate::Method>(self, _req: &M::Req) -> Replier<'a, T, M> {
        Replier::new(self.client)
    }
    pub fn reply<TransportError, Error>(
        self,
        res: Method::Res,
    ) -> impl Future<
        Output = Result<ReplyReceipt<Method::Res>, super::ClientError<TransportError, Error>>,
    > + Send
    where
        Method::Res: Sync + RpcMessage,
    {
        async {
            assert!(res.cbor_len(&mut ()) <= Method::Res::max_len());
            let written = self.client.write(&res).await;
            written
                .map(move |_| ReplyReceipt(res))
                .map_err(|e| super::ClientError::Rpc(super::RpcError::from(e)))
        }
    }

    pub fn reply_with<TransportError>(
        self,
        handler: &mut Method,
        req: Method::Req,
    ) -> impl Future<
        Output = Result<
            ReplyReceipt<Method::Res>,
            super::ClientError<TransportError, Method::Error>,
        >,
    >
    where
        Method: crate::Call,
        Method::Res: Send + Sync,
    {
        async { handler.call(self, req).await }
    }
}

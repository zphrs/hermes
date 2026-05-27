use futures::AsyncWrite;
use maxlen::MaxLen;
use minicbor::CborLen as _;

use crate::Method;

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

pub struct Replier<'a, T: AsyncWrite + Unpin + Send + Sync> {
    client: &'a mut minicbor_io::AsyncWriter<T>,
}

impl<'a, T: AsyncWrite + Unpin + Send + Sync> Replier<'a, T> {
    pub(crate) fn new(client: &'a mut minicbor_io::AsyncWriter<T>) -> Self {
        Self { client }
    }
    pub fn reply<TransportError, Error, M: Method + ?Sized>(
        self,
        res: M::Res,
    ) -> impl Future<
        Output = Result<ReplyReceipt<M::Res>, super::ClientError<TransportError, Error>>,
    > + Send
    where
        M::Res: Sync,
    {
        async {
            assert!(res.cbor_len(&mut ()) <= M::Res::max_len());
            let written = self.client.write(&res).await;
            written
                .map(move |_| ReplyReceipt(res))
                .map_err(|e| super::ClientError::Rpc(super::RpcError::from(e)))
        }
    }

    pub fn reply_with<TransportError: Send, Error, M: crate::Call>(
        self,
        handler: &mut M,
        req: M::Req,
    ) -> impl Future<Output = Result<ReplyReceipt<M::Res>, super::ClientError<TransportError, Error>>>
    where
        <M as Method>::Res: Send + Sync,
        Error: From<M::Error>,
    {
        async {
            self.reply::<_, _, M>(
                handler
                    .call(req)
                    .await
                    .map_err(|e| super::ClientError::App(Error::from(e)))?,
            )
            .await
        }
    }
}

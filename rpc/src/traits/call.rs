use futures::AsyncWrite;

use crate::{Method, ReplyReceipt};

pub trait Call: Method {
    fn call<T: AsyncWrite + Unpin + Send + Sync, TransportError>(
        &mut self,
        replier: crate::Replier<'_, T, Self>,
        value: Self::Req,
    ) -> impl Future<
        Output = Result<ReplyReceipt<Self::Res>, crate::ClientError<TransportError, Self::Error>>,
    > + Send;
}

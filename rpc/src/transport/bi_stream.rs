use futures::{AsyncRead, AsyncWrite};

pub trait BiStream {
    type RecvStream: AsyncRead + Unpin + Send + Sync;
    type SendStream: AsyncWrite + Unpin + Send + Sync;
}

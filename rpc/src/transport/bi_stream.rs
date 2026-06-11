use futures::{AsyncRead, AsyncWrite};

pub trait BiStream {
    type RecvStream: AsyncRead + Unpin;
    type SendStream: AsyncWrite + Unpin;
}

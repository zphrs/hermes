use futures::{AsyncRead, AsyncWrite};

use crate::transport::Close;

pub trait BiStream {
    type RecvStream: AsyncRead + Unpin + Send + Sync;
    type SendStream: AsyncWrite + Unpin + Send + Sync;
}

pub struct StreamPair<Bs: BiStream>(pub Bs::SendStream, pub Bs::RecvStream);

impl<Bs: BiStream> From<(Bs::SendStream, Bs::RecvStream)> for StreamPair<Bs> {
    fn from((send, recv): (Bs::SendStream, Bs::RecvStream)) -> Self {
        StreamPair(send, recv)
    }
}
impl<Bs: BiStream> StreamPair<Bs> {
    fn write_mut(&mut self) -> &mut Bs::SendStream {
        &mut self.0
    }

    fn read_mut(&mut self) -> &mut Bs::RecvStream {
        &mut self.1
    }
}

pub struct InitializedMessageStream<Bs: BiStream>(StreamPair<Bs>);

impl<Bs: BiStream> InitializedMessageStream<Bs> {
    pub fn new(stream_pair: StreamPair<Bs>) -> Self {
        Self(stream_pair)
    }

    pub fn write_mut(&mut self) -> &mut Bs::SendStream {
        self.0.write_mut()
    }

    pub fn read_mut(&mut self) -> &mut Bs::RecvStream {
        self.0.read_mut()
    }

    pub fn close(self) -> impl Future<Output = ()>
    where
        Bs::SendStream: Close,
    {
        async { self.0.0.close().await }
    }
}

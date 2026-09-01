use core::fmt::Debug;
use std::fmt::Display;

use bytes::Bytes;

use quinn::{ConnectionError, ReadError, WriteError};
pub trait BytesReadStream {
    type Error: Debug + Display;
    fn try_next(&mut self) -> impl Future<Output = Result<Option<Bytes>, Self::Error>>;
}

pub trait BytesWriteStream: Stopped {
    type Error: Debug;
    fn try_put(&mut self, bytes: Bytes) -> impl Future<Output = Result<(), Self::Error>>;
}

pub trait Stopped {
    fn finish(&mut self) -> ();
    fn stopped(&self) -> impl Future<Output = ()>;
}
pub trait Connection {
    type SendStream: BytesWriteStream + Stopped;
    type RecvStream: BytesReadStream;
    type OpenError;
    fn open_stream(
        &self,
    ) -> impl Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::OpenError>>;

    type AcceptError;
    fn accept_stream(
        &self,
    ) -> impl Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::AcceptError>>;

    type OpenUniError;
    fn open_uni_stream(&self)
    -> impl Future<Output = Result<Self::SendStream, Self::OpenUniError>>;

    type AcceptUniError;
    fn accept_uni_stream(
        &self,
    ) -> impl Future<Output = Result<Self::RecvStream, Self::AcceptUniError>>;

    type Id: PartialEq;
    fn stable_id(&self) -> Self::Id;

    type CloseError;
    fn wait_for_close(self) -> impl Future<Output = Result<(), Self::CloseError>>;
}

impl Stopped for quinn::SendStream {
    async fn stopped(&self) {
        let _ = quinn::SendStream::stopped(self).await;
    }

    fn finish(&mut self) {
        let _ = quinn::SendStream::finish(self);
    }
}

impl Connection for quinn::Connection {
    type SendStream = quinn::SendStream;
    type RecvStream = quinn::RecvStream;

    type OpenError = quinn::ConnectionError;
    fn open_stream(
        &self,
    ) -> impl Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::OpenError>> {
        self.open_bi()
    }

    type AcceptError = quinn::ConnectionError;
    fn accept_stream(
        &self,
    ) -> impl Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::AcceptError>> {
        self.accept_bi()
    }

    type OpenUniError = quinn::ConnectionError;
    fn open_uni_stream(
        &self,
    ) -> impl Future<Output = Result<Self::SendStream, Self::OpenUniError>> {
        self.open_uni()
    }

    type AcceptUniError = quinn::ConnectionError;
    fn accept_uni_stream(
        &self,
    ) -> impl Future<Output = Result<Self::RecvStream, Self::AcceptUniError>> {
        self.accept_uni()
    }

    type Id = usize;

    fn stable_id(&self) -> Self::Id {
        self.stable_id()
    }

    type CloseError = ConnectionError;
    async fn wait_for_close(self) -> Result<(), Self::CloseError> {
        self.closed().await;
        Ok(())
    }
}

impl BytesReadStream for quinn::RecvStream {
    type Error = ReadError;

    async fn try_next(&mut self) -> Result<Option<Bytes>, Self::Error> {
        let value = self.read_chunk(usize::MAX, true).await?;
        Ok(value.map(|chunk| chunk.bytes))
    }
}

impl BytesWriteStream for quinn::SendStream {
    type Error = WriteError;

    fn try_put(&mut self, bytes: Bytes) -> impl Future<Output = Result<(), Self::Error>> {
        self.write_chunk(bytes)
    }
}

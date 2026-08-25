use quinn::ConnectionError;

use crate::transport::BiStream;

impl BiStream for quinn::Connection {
    type RecvStream = quinn::RecvStream;

    type SendStream = quinn::SendStream;
}

impl crate::Caller for quinn::Connection {
    type Error = ConnectionError;

    async fn open_stream(&self) -> Result<(Self::SendStream, Self::RecvStream), Self::Error> {
        self.open_bi().await
    }
}
impl crate::transport::Client for quinn::Connection {
    type Error = ConnectionError;

    async fn accept_stream(&self) -> Result<(Self::SendStream, Self::RecvStream), Self::Error> {
        self.accept_bi().await
    }
}
impl crate::transport::Incoming for quinn::Incoming {
    type Client = quinn::Connection;

    type Error = ConnectionError;

    async fn accept(self) -> Result<Self::Client, Self::Error> {
        self.accept()?.await
    }
}

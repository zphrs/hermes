pub mod transport {
    use std::{net::SocketAddr, sync::Arc};

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("connect: {0}")]
        Connect(#[from] quinn::ConnectError),
        #[error("connection: {0}")]
        Connection(#[from] quinn::ConnectionError),
        #[error("endpoint closed")]
        EndpointClosed,
    }

    pub struct Address {
        pub config: quinn::ClientConfig,
        pub addr: SocketAddr,
        pub server_name: Arc<str>,
    }

    impl crate::Transport for quinn::Endpoint {
        type Address = Address;

        type Error = Error;

        type Caller = quinn::Connection;

        async fn connect(
            &self,
            Address {
                config,
                addr,
                server_name,
            }: &Self::Address,
        ) -> Result<Self::Caller, Self::Error> {
            Ok(self
                .connect_with(config.clone(), *addr, server_name.as_ref())?
                .await?)
        }

        type Client = quinn::Connection;

        type Incoming = quinn::Incoming;

        async fn accept(&self) -> Result<Self::Incoming, Self::Error> {
            self.accept().await.ok_or(Error::EndpointClosed)
        }
    }
}

pub mod connection {
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
}

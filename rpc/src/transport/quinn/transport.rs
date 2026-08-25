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

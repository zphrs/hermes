mod skip_server_verification;

use skip_server_verification::SkipServerVerification;

use std::{
    collections::HashMap,
    net::{Ipv6Addr, SocketAddr},
    sync::Arc,
};

use quinn::{ServerConfig, crypto::rustls::QuicClientConfig};
use rpc::BiStream;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use shared_schema::sky_node::SkyId;

use crate::{get_public_ip, request_handler::PORT};

pub struct Transport {
    endpoint: quinn::Endpoint,
    open_conns: HashMap<SocketAddr, quinn::Connection>,
}

pub fn url_from_id(id: &SkyId) -> std::string::String {
    let id = id.to_string();
    let left = &id[..id.len() / 2];
    let right = &id[id.len() / 2..];
    format!("{left}.{right}.invalid")
}

impl Transport {
    pub async fn self_signed_server() -> Result<Self, Error> {
        let public_ip = get_public_ip().await.ok_or(Error::NoPublicIp)?;
        let hashed_id = SkyId::from(public_ip);

        let url = url_from_id(&hashed_id);

        let cert = rcgen::generate_simple_self_signed(vec![url])?;
        let cert_der = CertificateDer::from(cert.cert);
        let key = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
        Self::bind_with_cert(
            vec![cert_der],
            key.into(),
            SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), PORT),
        )
    }

    pub async fn client() -> Result<Self, Error> {
        let mut endpoint =
            quinn::Endpoint::client(SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0))?;
        endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(
            QuicClientConfig::try_from(
                rustls::ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(SkipServerVerification::new())
                    .with_no_client_auth(),
            )
            .unwrap(),
        )));
        Ok(Self {
            endpoint,
            open_conns: Default::default(),
        })
    }

    /// id should be based on current public ip
    fn bind_with_cert(
        certs: Vec<CertificateDer<'static>>,
        priv_key: PrivateKeyDer<'static>,
        addr: SocketAddr,
    ) -> Result<Self, Error> {
        let endpoint =
            quinn::Endpoint::server(ServerConfig::with_single_cert(certs, priv_key)?, addr)?;
        Ok(Self {
            endpoint,
            open_conns: Default::default(),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("connect: {0}")]
    Connect(#[from] quinn::ConnectError),
    #[error("connection: {0}")]
    Connection(#[from] quinn::ConnectionError),
    #[error("rustls: {0}")]
    Rustls(#[from] rustls::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("rcgen: {0}")]
    RcGen(#[from] rcgen::Error),
    #[error("no public ip")]
    NoPublicIp,
    #[error("connection closed")]
    ConnectionClosed,
}

impl rpc::Transport for Transport {
    type Address = SocketAddr;

    type Error = Error;

    type Caller = Caller;

    async fn connect(&mut self, to: Self::Address) -> Result<Self::Caller, Self::Error> {
        let conn = match self.open_conns.get_mut(&to) {
            Some(c) => c,
            None => {
                let id = SkyId::from(to.ip());

                let url = url_from_id(&id);
                let new_conn = self.endpoint.connect(to, &url)?.await?;
                self.open_conns.entry(to).or_insert(new_conn)
            }
        };
        // Connection: May be cloned to obtain another handle to the same connection.
        Ok(quinn::Connection::clone(conn).into())
    }

    type Client = Client;

    async fn accept(&mut self) -> Result<Self::Client, Self::Error> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or(Error::ConnectionClosed)?;
        let conn = incoming.await?;
        Ok(Self::Client::from(conn))
    }
}

pub struct Caller {
    conn: quinn::Connection,
}

impl From<quinn::Connection> for Caller {
    fn from(conn: quinn::Connection) -> Self {
        Self { conn }
    }
}

impl BiStream for Caller {
    type RecvStream = quinn::RecvStream;

    type SendStream = quinn::SendStream;
}

impl rpc::Caller for Caller {
    type Error = Error;

    async fn open_stream(&mut self) -> Result<(Self::SendStream, Self::RecvStream), Self::Error> {
        self.conn.open_bi().await.map_err(Self::Error::from)
    }
}

pub struct Client {
    conn: quinn::Connection,
}

impl From<quinn::Connection> for Client {
    fn from(conn: quinn::Connection) -> Self {
        Self { conn }
    }
}

impl BiStream for Client {
    type RecvStream = quinn::RecvStream;
    type SendStream = quinn::SendStream;
}

impl rpc::Client for Client {
    type Error = Error;

    async fn accept_stream(&mut self) -> Result<(Self::SendStream, Self::RecvStream), Self::Error> {
        Ok(self.conn.accept_bi().await?)
    }
}

mod end_to_end_socket;
mod skip_server_verification;

use public_ip::addr;
use skip_server_verification::SkipServerVerification;

#[cfg(test)]
pub use end_to_end_test::UdpSocket;
#[cfg(not(test))]
pub use tokio::net::UdpSocket;

use std::{
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
};

use quinn::{ServerConfig, crypto::rustls::QuicClientConfig};
use rpc::BiStream;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use shared_schema::sky_node::SkyId;

use crate::{quinn::end_to_end_socket::EndToEndSocket, request_handler::PORT};

pub struct Transport {
    endpoint: quinn::Endpoint,
    open_conns: HashMap<SocketAddr, quinn::Connection>,
}

#[cfg(not(test))]
pub use crate::get_public_ip::get_public_ip;
#[cfg(test)]
pub use crate::get_public_ip::get_public_ip_mock as get_public_ip;

impl Transport {
    pub async fn self_signed_server() -> Result<Self, Error> {
        let public_ip = get_public_ip().await.ok_or(Error::NoPublicIp)?;
        let hashed_id = SkyId::from(public_ip);

        let url = hashed_id.to_url();

        let cert = rcgen::generate_simple_self_signed(vec![url])?;
        let cert_der = CertificateDer::from(cert.cert);
        let key = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
        Self::bind_with_cert(
            vec![cert_der],
            key.into(),
            SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), PORT),
        )
        .await
    }

    pub async fn client() -> Result<Self, Error> {
        let sock = UdpSocket::bind(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0)).await?;
        let abstract_sock = EndToEndSocket::from(sock);
        let mut endpoint = quinn::Endpoint::new_with_abstract_socket(
            Default::default(),
            None,
            Arc::new(abstract_sock),
            Arc::new(quinn::TokioRuntime),
        )?;
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
    async fn bind_with_cert(
        certs: Vec<CertificateDer<'static>>,
        priv_key: PrivateKeyDer<'static>,
        addr: SocketAddr,
    ) -> Result<Self, Error> {
        let sock = UdpSocket::bind(addr).await?;
        let abstract_sock = EndToEndSocket::from(sock);
        let endpoint = quinn::Endpoint::new_with_abstract_socket(
            Default::default(),
            Some(ServerConfig::with_single_cert(certs, priv_key)?),
            Arc::new(abstract_sock),
            Arc::new(quinn::TokioRuntime),
        )?;
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

                let url = id.to_url();
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

impl rpc::Close for Transport {
    async fn close(self) {
        self.endpoint.wait_idle().await
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

#[cfg(test)]
mod tests {
    use rpc::{Call, Caller, Client, Close, Transport as _};
    use std::{convert::Infallible, time::Duration};
    use tracing::{Instrument, Level, info, span};
    use tracing::{instrument, trace};
    use tracing_test::traced_test;

    use end_to_end_test::{Host, Machine, OsShim, net::ip, sim::Sim};
    use shared_schema::ping;

    use crate::{quinn::Transport, request_handler::PORT};

    struct PingHandler;

    impl rpc::RootHandler<shared_schema::ping::Request> for PingHandler {
        type Error = Infallible;

        async fn handle<T: futures_io::AsyncWrite + Unpin + Sync + Send, TransportError: Send>(
            &mut self,
            root: shared_schema::ping::Request,
            replier: rpc::Replier<'_, T>,
        ) -> Result<rpc::ReplyReceipt, rpc::ClientError<TransportError, Self::Error>> {
            trace!("Handling client");
            let res = ping::Method.call(root).await?;
            let out = replier.reply(res).await?;
            trace!("finished handling client");
            Ok(out)
        }
    }

    #[test]
    #[traced_test]
    pub fn quinn() {
        let sim = Sim::new();
        sim.enter_runtime(|| {
            let net = Sim::add_machine(ip::Network::new());
            let server = OsShim::new(Host::new(10, move || {
                let span = span!(Level::TRACE, "server");
                async {
                    let mut tp = Transport::self_signed_server().await?;
                    trace!("server inited");

                    let mut client = tp.accept().await?;
                    trace!("accepted client conn");
                    client.handle_one_request(&mut PingHandler).await?;

                    trace!("handled client");
                    tp.close().await;

                    Ok(())
                }
                .instrument(span)
            }));

            let server_addr = server.get().borrow().connect_to_net(net);
            server.get().borrow().set_public_ip(server_addr);
            Sim::tick_machine(server).unwrap();

            let client = OsShim::new(Host::new(10, move || {
                let span = span!(Level::TRACE, "client");
                async move {
                    let mut tp = Transport::client().await?;
                    trace!("inited");
                    let mut conn = tp.connect((server_addr, PORT).into()).await?;
                    conn.query::<shared_schema::ping::Method, shared_schema::ping::Request>(
                        shared_schema::ping::Request,
                    )
                    .await?;
                    trace!("got response");

                    tp.close().await;

                    Ok(())
                }
                .instrument(span)
            }));
            let client_ip = client.get().borrow().connect_to_net(net);
            client.get().borrow().set_public_ip(client_ip);
            Sim::run_until_idle().unwrap();
        })
    }
}

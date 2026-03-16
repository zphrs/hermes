mod end_to_end_socket;
mod skip_server_verification;

use end_to_end_test::sim::RNG;
use quinn::{
    ConnectionId, ConnectionIdGenerator, EndpointStats,
    congestion::{Cubic, CubicConfig},
};
use rand::Rng;
use sha2::digest::generic_array::arr::Inc;
use skip_server_verification::SkipServerVerification;

#[cfg(test)]
pub use end_to_end_test::UdpSocket;
#[cfg(not(test))]
pub use tokio::net::UdpSocket;
use tokio::{runtime::Handle, time::sleep_until};
use tracing::{Instrument as _, Level, span, trace, warn};

use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
    u64, usize,
};

use quinn::{EndpointConfig, Runtime, ServerConfig, VarInt, crypto::rustls::QuicClientConfig};
use rpc::BiStream;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use shared_schema::sky_node::SkyId;

use crate::request_handler::PORT;

pub struct Transport {
    endpoint: quinn::Endpoint,
    open_conns: HashMap<SocketAddr, quinn::Connection>,
}

impl Transport {
    pub fn stats(&self) -> EndpointStats {
        self.endpoint.stats()
    }
}

#[cfg(not(test))]
pub use crate::get_public_ip::get_public_ip;
#[cfg(test)]
pub use crate::get_public_ip::get_public_ip_mock as get_public_ip;

#[derive(Debug)]
struct TokioRuntime;

struct Timesource {
    start: std::time::Instant,
}

impl Timesource {
    fn new() -> Self {
        Self {
            start: TokioRuntime.now(),
        }
    }
}

impl quinn::TimeSource for Timesource {
    fn now(&self) -> std::time::SystemTime {
        let now = TokioRuntime.now();
        let diff = now.duration_since(self.start);
        std::time::SystemTime::UNIX_EPOCH + diff
    }
}

struct SeededCidGenerator;

impl ConnectionIdGenerator for SeededCidGenerator {
    fn generate_cid(&mut self) -> ConnectionId {
        RNG.with(|rng| {
            let mut rng = rng.borrow_mut();
            ConnectionId::new(&rng.random::<[u8; 20]>())
        })
    }

    fn cid_len(&self) -> usize {
        20
    }

    fn cid_lifetime(&self) -> Option<Duration> {
        None
    }
}

impl quinn::Runtime for TokioRuntime {
    fn new_timer(&self, i: std::time::Instant) -> std::pin::Pin<Box<dyn quinn::AsyncTimer>> {
        let sleeping_for = i.saturating_duration_since(self.now());

        trace!("quinn sleeping for {:?}", sleeping_for);
        Box::pin(sleep_until(i.into()))
    }

    fn spawn(&self, future: std::pin::Pin<Box<dyn Future<Output = ()> + Send>>) {
        tokio::spawn(future);
    }

    fn wrap_udp_socket(
        &self,
        t: std::net::UdpSocket,
    ) -> std::io::Result<Arc<dyn quinn::AsyncUdpSocket>> {
        unimplemented!()
    }

    fn now(&self) -> std::time::Instant {
        // panic if not in a tokio runtime
        let rt = Handle::current();
        let _g = rt.enter();
        let out = tokio::time::Instant::now().into_std();
        out
    }
}

impl Transport {
    fn transport_config() -> Arc<quinn::TransportConfig> {
        let mut transport = quinn::TransportConfig::default();
        transport.max_idle_timeout(Some(VarInt::from(30_000u32).into()));
        // transport.keep_alive_interval(Some(Duration::new(15, 0)));
        // transport.receive_window(VarInt::MAX);
        // transport.stream_receive_window(VarInt::MAX);
        // transport.send_window(u64::MAX);
        transport.send_fairness(false);
        // transport.max_concurrent_bidi_streams(10_000u32.into());
        // transport.congestion_controller_factory(Arc::new(
        //     CubicConfig::default()
        //         .initial_window(20_000_000_000)
        //         .clone(),
        // ));
        Arc::new(transport)
    }

    fn endpoint_config() -> quinn::EndpointConfig {
        let mut endpoint_config: EndpointConfig = Default::default();
        endpoint_config.rng_seed(Some(RNG.with(|rng| rng.borrow_mut().random())));
        endpoint_config.cid_generator(|| Box::new(SeededCidGenerator));
        endpoint_config
    }

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
        let addr = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0);
        #[cfg(test)]
        let abstract_sock = {
            use super::quinn::end_to_end_socket::EndToEndSocket;
            let sock = UdpSocket::bind(addr).await?;
            Arc::new(EndToEndSocket::from(sock))
        };
        #[cfg(not(test))]
        let abstract_sock = {
            use quinn::Runtime;
            quinn::TokioRuntime.wrap_udp_socket(std::net::UdpSocket::bind(addr)?)?
        };

        let mut endpoint_config = Self::endpoint_config();

        let mut endpoint = quinn::Endpoint::new_with_abstract_socket(
            endpoint_config,
            None,
            abstract_sock,
            Arc::new(TokioRuntime),
        )?;
        let mut client_config = quinn::ClientConfig::new(Arc::new(
            QuicClientConfig::try_from(
                rustls::ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(SkipServerVerification::new())
                    .with_no_client_auth(),
            )
            .unwrap(),
        ));
        let transport = Self::transport_config();
        client_config.transport_config(transport);
        endpoint.set_default_client_config(client_config);
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
        #[cfg(test)]
        let abstract_sock = {
            use super::quinn::end_to_end_socket::EndToEndSocket;

            let sock = UdpSocket::bind(addr).await?;
            Arc::new(EndToEndSocket::from(sock))
        };
        #[cfg(not(test))]
        let abstract_sock = {
            use quinn::Runtime;
            quinn::TokioRuntime.wrap_udp_socket(std::net::UdpSocket::bind(addr)?)?
        };
        let mut server_config = ServerConfig::with_single_cert(certs, priv_key)?;
        server_config.time_source(Arc::new(Timesource::new()));
        server_config.max_incoming(usize::MAX);
        server_config.incoming_buffer_size_total(u64::MAX);

        let transport = Self::transport_config();

        server_config.transport_config(transport);
        let endpoint_config = Self::endpoint_config();
        let endpoint = quinn::Endpoint::new_with_abstract_socket(
            endpoint_config,
            Some(server_config),
            abstract_sock,
            Arc::new(TokioRuntime),
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

    async fn accept(&mut self) -> Result<Self::Incoming, Self::Error> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or(Error::ConnectionClosed)?;
        Ok(Incoming::from(incoming))
    }
    type Incoming = Incoming;
}

pub struct Incoming {
    incoming: quinn::Incoming,
}

impl From<quinn::Incoming> for Incoming {
    fn from(incoming: quinn::Incoming) -> Self {
        Self { incoming }
    }
}

impl rpc::Incoming for Incoming {
    type Client = Client;
    type Error = Error;
    async fn accept(self) -> Result<Self::Client, Self::Error> {
        Ok(Client::from(self.incoming.await?))
    }
}

impl rpc::Close for Transport {
    async fn close(self) {
        self.endpoint.close(0u32.into(), b"");
        self.endpoint.wait_idle().await
    }
}

#[derive(Clone)]
pub struct Caller {
    conn: quinn::Connection,
}

impl rpc::Close for Caller {
    async fn close(self) {
        self.conn.close(VarInt::from_u32(0), b"");
    }
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

impl Client {
    pub fn stats(&self) -> quinn::ConnectionStats {
        self.conn.stats()
    }
}

impl Caller {
    pub fn stats(&self) -> quinn::ConnectionStats {
        self.conn.stats()
    }
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

impl rpc::Close for Client {
    async fn close(self) {
        self.conn.close(VarInt::from_u32(0), b"");
    }
}

#[cfg(test)]
mod tests {

    use rpc::{Call, Caller, Client, Close, Incoming, Transport as _};
    use std::convert::Infallible;
    use tracing::trace;
    use tracing::{Instrument, Level, span};

    use end_to_end_test::{Host, OsShim, net::ip, sim::Sim};
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

    #[test_log::test]
    pub fn quinn() {
        let sim = Sim::new();
        sim.enter_runtime(|| {
            let net = Sim::add_machine(ip::Network::new());
            let server = OsShim::new(Host::new(move || {
                let span = span!(Level::TRACE, "server");
                async {
                    let mut tp = Transport::self_signed_server().await?;
                    trace!("server inited");

                    let mut client = tp.accept().await?.accept().await?;
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

            let client = OsShim::new(Host::new(move || {
                let span = span!(Level::TRACE, "client");
                async move {
                    trace!("running");

                    let mut tp = Transport::client().await?;
                    let mut conn = tp.connect((server_addr, PORT).into()).await?;
                    conn.query::<shared_schema::ping::Method, shared_schema::ping::Request>(
                        shared_schema::ping::Request,
                    )
                    .await?;
                    trace!("got response");

                    conn.close().await;

                    Ok(())
                }
                .instrument(span)
            }));
            let client_ip = client.get().borrow().connect_to_net(net);
            client.get().borrow().set_public_ip(client_ip);
            let arr = [client, server];
            Sim::run_until_idle(|| arr.iter()).unwrap();
        })
    }
}

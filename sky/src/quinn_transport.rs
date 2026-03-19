#[cfg(test)]
mod end_to_end_socket;
mod skip_server_verification;

#[cfg(test)]
use end_to_end_test::sim::RNG;
use quinn::{ConnectionId, ConnectionIdGenerator, EndpointStats};
use rand::Rng;

use skip_server_verification::SkipServerVerification;

#[cfg(test)]
pub use end_to_end_test::UdpSocket;
#[cfg(not(test))]
pub use tokio::net::UdpSocket;
use tokio::{runtime::Handle, time::sleep_until};
use tracing::{info, trace, warn};

use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    ops::Deref,
    sync::Arc,
    time::Duration,
    u64, usize,
};

use quinn::{EndpointConfig, Runtime, ServerConfig, VarInt, crypto::rustls::QuicClientConfig};
use rpc::BiStream;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use shared_schema::{
    SkyNode,
    sky_node::{PORT, SkyId},
};

#[derive(Clone)]
pub struct Transport {
    endpoint: quinn::Endpoint,
    open_conns: Arc<
        tokio::sync::RwLock<HashMap<SocketAddr, tokio::sync::Mutex<Option<quinn::Connection>>>>,
    >,
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

#[cfg(test)]
mod test_utils {
    use std::{sync::Arc, time::Duration};

    use end_to_end_test::sim::RNG;
    use quinn::{ConnectionId, ConnectionIdGenerator, Runtime as _};
    use rand::Rng as _;
    use tokio::time::sleep_until;
    use tracing::trace;
    #[derive(Debug)]
    pub struct TokioRuntime;

    pub struct Timesource {
        start: std::time::Instant,
    }

    impl Timesource {
        pub fn new() -> Self {
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

    pub struct SeededCidGenerator;

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
            _t: std::net::UdpSocket,
        ) -> std::io::Result<Arc<dyn quinn::AsyncUdpSocket>> {
            unimplemented!()
        }

        fn now(&self) -> std::time::Instant {
            // panic if not in a tokio runtime
            let rt = tokio::runtime::Handle::current();
            let _g = rt.enter();
            let out = tokio::time::Instant::now().into_std();
            out
        }
    }
}

impl Transport {
    fn transport_config() -> quinn::TransportConfig {
        let mut transport = quinn::TransportConfig::default();
        transport.max_idle_timeout(Some(VarInt::from(30_000u32).into()));
        transport.send_fairness(false);
        transport
    }

    fn endpoint_config() -> quinn::EndpointConfig {
        #[cfg(test)]
        {
            let mut endpoint_config: EndpointConfig = Default::default();
            endpoint_config.rng_seed(Some(RNG.with(|rng| rng.borrow_mut().random())));
            endpoint_config.cid_generator(|| Box::new(test_utils::SeededCidGenerator));
            endpoint_config
        }
        #[cfg(not(test))]
        {
            let endpoint_config: EndpointConfig = Default::default();
            endpoint_config
        }
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
        Self::client_with_keepalive(true).await
    }

    pub async fn client_with_keepalive(keepalive: bool) -> Result<Self, Error> {
        let addr = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0);
        #[cfg(test)]
        let abstract_sock = {
            use super::quinn_transport::end_to_end_socket::EndToEndSocket;
            let sock = UdpSocket::bind(addr).await?;
            Arc::new(EndToEndSocket::from(sock))
        };
        #[cfg(not(test))]
        let abstract_sock = {
            use quinn::Runtime;
            quinn::TokioRuntime.wrap_udp_socket(std::net::UdpSocket::bind(addr)?)?
        };

        let endpoint_config = Self::endpoint_config();

        let mut endpoint = quinn::Endpoint::new_with_abstract_socket(
            endpoint_config,
            None,
            abstract_sock,
            #[cfg(test)]
            Arc::new(test_utils::TokioRuntime),
            #[cfg(not(test))]
            Arc::new(quinn::TokioRuntime),
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
        let mut transport = Self::transport_config();
        if keepalive {
            transport.keep_alive_interval(Some(Duration::new(10, 0)));
        }
        client_config.transport_config(transport.into());
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
            use super::quinn_transport::end_to_end_socket::EndToEndSocket;

            let sock = UdpSocket::bind(addr).await?;
            Arc::new(EndToEndSocket::from(sock))
        };
        #[cfg(not(test))]
        let abstract_sock = {
            use quinn::Runtime;
            quinn::TokioRuntime.wrap_udp_socket(std::net::UdpSocket::bind(addr)?)?
        };
        let mut server_config = ServerConfig::with_single_cert(certs, priv_key)?;
        #[cfg(test)]
        server_config.time_source(Arc::new(test_utils::Timesource::new()));
        server_config.max_incoming(usize::MAX);
        server_config.incoming_buffer_size_total(u64::MAX);

        let transport = Self::transport_config();

        server_config.transport_config(transport.into());
        let endpoint_config = Self::endpoint_config();
        let endpoint = quinn::Endpoint::new_with_abstract_socket(
            endpoint_config,
            Some(server_config),
            abstract_sock,
            #[cfg(test)]
            Arc::new(test_utils::TokioRuntime),
            #[cfg(not(test))]
            Arc::new(quinn::TokioRuntime),
        )?;
        info!("hosting on {:?}", endpoint.local_addr());
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
    type Address = SkyNode;

    type Error = Error;

    type Caller = Caller;

    fn connect(
        &self,
        to: &Self::Address,
    ) -> impl Future<Output = Result<Self::Caller, Self::Error>> + Send {
        async {
            let mut read_lock = self.open_conns.read().await;
            let maybe_conn_mutex = read_lock.get(&to.socket_address());
            if let Some(conn_mutex) = maybe_conn_mutex {
                let conn = conn_mutex.lock().await;
                if let Some(conn) = &*conn
                    && conn.close_reason().is_none()
                {
                    return Ok(conn.clone().into());
                }
            }

            // now we need to hold lock on open_conns[to.socket_address()] so we only init a conn once
            let conn_mutex = match maybe_conn_mutex {
                Some(m) => m,
                None => {
                    // implicit maybe_conn_mutex drop
                    drop(read_lock);
                    let mut write_lock = self.open_conns.write().await;
                    write_lock.insert(to.socket_address(), Default::default());
                    read_lock = tokio::sync::RwLockWriteGuard::downgrade(write_lock);
                    read_lock.get(&to.socket_address()).unwrap()
                }
            };

            let mut conn_lock = conn_mutex.lock().await;

            let url = to.sky_id().to_url();
            let new_conn = self.endpoint.connect(to.socket_address(), &url)?.await?;
            *conn_lock = Some(new_conn.clone());
            Ok(new_conn.into())
        }
    }

    type Client = Client;

    async fn accept(&self) -> Result<Self::Incoming, Self::Error> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or(Error::ConnectionClosed)?;
        Ok(Incoming::from(incoming))
    }
    type Incoming = Incoming;
}

impl Deref for Transport {
    type Target = quinn::Endpoint;

    fn deref(&self) -> &Self::Target {
        &self.endpoint
    }
}

pub struct Incoming {
    incoming: quinn::Incoming,
}

impl Deref for Incoming {
    type Target = quinn::Incoming;

    fn deref(&self) -> &Self::Target {
        &self.incoming
    }
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
        // self.endpoint.close(VarInt::from_u32(0), b"");
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

    async fn open_stream(&self) -> Result<(Self::SendStream, Self::RecvStream), Self::Error> {
        self.conn.open_bi().await.map_err(Self::Error::from)
    }
}

pub struct Client {
    conn: quinn::Connection,
}

impl Deref for Client {
    type Target = quinn::Connection;

    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}

impl Deref for Caller {
    type Target = quinn::Connection;

    fn deref(&self) -> &Self::Target {
        &self.conn
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

    async fn accept_stream(&self) -> Result<(Self::SendStream, Self::RecvStream), Self::Error> {
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

    use expect_test::expect;
    use rpc::{Call, Caller, Client, Close, Incoming, Transport as _};
    use std::convert::Infallible;

    use std::time::Duration;
    use tracing::trace;
    use tracing::{Instrument, Level, span};

    use end_to_end_test::{Host, OsShim, net::ip, sim::Sim};
    use shared_schema::ping;

    use crate::quinn_transport::Transport;

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
    pub fn basic_quinn() {
        let sim = Sim::new();
        sim.enter_runtime(|| {
            let net = Sim::add_machine(ip::Network::new());
            let server = OsShim::new(Host::new(move || {
                let span = span!(Level::TRACE, "server");
                async {
                    let tp = Transport::self_signed_server().await?;
                    trace!("server inited");

                    let client = tp.accept().await?.accept().await?;
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

                    let tp = Transport::client().await?;
                    let conn = tp.connect(&server_addr.into()).await?;
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
    #[test_log::test]
    pub fn check_timeout_state() {
        let sim = Sim::new();
        sim.enter_runtime(|| {
            let net = Sim::add_machine(ip::Network::new());
            let server = OsShim::new(Host::new(move || {
                let span = span!(Level::TRACE, "server");
                async {
                    let tp = Transport::self_signed_server().await?;
                    let client = tp.accept().await?.accept().await?;
                    client.handle_one_request(&mut PingHandler).await?;
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
                    let tp = Transport::client_with_keepalive(false).await?;
                    let conn = tp.connect(&server_addr.into()).await?;
                    conn.query::<shared_schema::ping::Method, shared_schema::ping::Request>(
                        shared_schema::ping::Request,
                    )
                    .await?;
                    {
                        let _guard = ip::Network::add_one_way_partition(
                            net,
                            Sim::get_current_machine::<OsShim>()
                                .borrow()
                                .public_ip()
                                .unwrap(),
                            Sim::get_current_machine::<OsShim>()
                                .borrow()
                                .public_ip()
                                .unwrap(),
                        );
                        tokio::time::sleep(Duration::from_secs(40)).await;
                    }
                    expect!["timed out"].assert_eq(&conn.conn.close_reason().unwrap().to_string());

                    Ok(())
                }
                .instrument(span)
            }));
            let client_ip = client.get().borrow().connect_to_net(net);
            client.get().borrow().set_public_ip(client_ip);
            let arr = [client];
            Sim::run_until_idle(|| arr.iter()).unwrap();
        })
    }
}

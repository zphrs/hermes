#[cfg(test)]
mod end_to_end_socket;
mod skip_server_verification;

#[cfg(test)]
use end_to_end_test::sim::RNG;
use quinn::EndpointStats;

use skip_server_verification::SkipServerVerification;

#[cfg(test)]
pub use end_to_end_test::UdpSocket;
#[cfg(not(test))]
pub use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tracing::{Instrument, debug, instrument};

use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
    u64, usize,
};

use quinn::{EndpointConfig, ServerConfig, VarInt, crypto::rustls::QuicClientConfig};
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
        transport.max_idle_timeout(Some(VarInt::from(15_000u32).into()));
        // transport.max_idle_timeout(None);

        transport.send_fairness(false);
        transport
    }

    fn client_config(keep_alive: bool) -> quinn::ClientConfig {
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
        if keep_alive {
            transport.keep_alive_interval(Some(Duration::new(10, 0)));
        }
        client_config.transport_config(transport.into());
        client_config
    }

    fn endpoint_config() -> quinn::EndpointConfig {
        #[cfg(test)]
        {
            use rand::Rng;
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
        Self::bind_with_cert(vec![cert_der], key.into(), SocketAddr::new(public_ip, PORT)).await
    }

    pub async fn client() -> Result<Self, Error> {
        Self::client_with_keepalive(false).await
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

        let client_config = Self::client_config(keepalive);
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
        let mut endpoint = quinn::Endpoint::new_with_abstract_socket(
            endpoint_config,
            Some(server_config),
            abstract_sock,
            #[cfg(test)]
            Arc::new(test_utils::TokioRuntime),
            #[cfg(not(test))]
            Arc::new(quinn::TokioRuntime),
        )?;
        endpoint.set_default_client_config(Self::client_config(false));

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
    #[instrument(skip(self))]
    async fn connect(&self, to: &Self::Address) -> Result<Self::Caller, Self::Error> {
        let cloned_self = self.clone();
        let to = to.clone();
        let mut read_lock = cloned_self.open_conns.clone().read_owned().await;
        if let Some(conn_mutex) = read_lock.get(&to.socket_address()) {
            let mut conn_lock = conn_mutex.lock().await;
            if let Some(conn) = &*conn_lock {
                if conn.close_reason().is_none() {
                    debug!("reusing connection");

                    return Ok(conn.clone().into());
                } else {
                    *conn_lock = None;
                }
            }
        }
        let jh: JoinHandle<Result<Self::Caller, Self::Error>> = tokio::spawn(
            async move {
                // now we need to get a lock on open_conns[to.socket_address()] so we only init a conn once
                let conn_mutex = match read_lock.get(&to.socket_address()) {
                    Some(m) => m,
                    None => {
                        // implicit maybe_conn_mutex drop
                        drop(read_lock);
                        debug!("opening a connection with {to:?}");
                        let mut write_lock = cloned_self.open_conns.write_owned().await;
                        debug!("got write lock");

                        // theoretically someone could have filled it because we dropped the read lock
                        // momentarily
                        write_lock.entry(to.socket_address()).or_default();
                        read_lock = tokio::sync::OwnedRwLockWriteGuard::downgrade(write_lock);
                        read_lock.get(&to.socket_address()).unwrap()
                    }
                };

                let mut conn_lock = conn_mutex.lock().await;
                // someone could have beaten us here
                if let Some(conn) = &*conn_lock {
                    return Ok(conn.clone().into());
                }
                debug!("creating new connection");

                let url = to.sky_id().to_url();
                let new_conn = cloned_self
                    .endpoint
                    .connect(to.socket_address(), &url)?
                    .await?;
                *conn_lock = Some(new_conn.clone());
                Ok(new_conn.into())
            }
            .instrument(tracing::span::Span::current()),
        );

        Ok(jh.await.unwrap()?)
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

impl Transport {
    pub fn inner(&self) -> &quinn::Endpoint {
        &self.endpoint
    }
}

pub struct Incoming {
    incoming: quinn::Incoming,
}

impl Incoming {
    pub fn inner(&self) -> &quinn::Incoming {
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
        self.conn.close(VarInt::from_u32(10), b"");
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

impl Client {
    pub fn inner(&self) -> &quinn::Connection {
        &self.conn
    }
}

impl Caller {
    pub fn inner(&self) -> &quinn::Connection {
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
        self.conn.close(VarInt::from_u32(20), b"");
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
            let out = replier
                .reply::<_, _, shared_schema::ping::Method>(res)
                .await?;
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

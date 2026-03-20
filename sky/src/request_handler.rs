use std::{
    borrow::Cow,
    convert::Infallible,
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use kademlia::{RoutingTable, node_cache::KadNodeCache};
use maxlen::MaxLen;

use minicbor::CborLen as _;
use rpc::{Call, Caller, Client as _, ClientError, Transport};
use shared_schema::{MaxSizedVec, SkyNode, ping, sky_node::SkyId};
use tokio::task::JoinSet;
use tracing::{Instrument as _, trace, trace_span};

use crate::quinn_transport::{self, get_public_ip};

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
#[cbor(flat)]
pub enum RootRequest<'a> {
    /// can be sent by anyone
    #[n(0)]
    Ping(#[n(0)] ping::Request),
    /// get nearby known sky nodes based on address
    #[n(1)]
    FindNodes(#[n(0)] FindNodesRequest<'a>),
}

impl<'a> From<FindNodesRequest<'a>> for RootRequest<'a> {
    fn from(value: FindNodesRequest<'a>) -> Self {
        Self::FindNodes(value)
    }
}

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
pub struct FindNodesRequest<'a> {
    #[n(0)]
    pub sky_id: SkyId,
    // sky nodes should always specify the sender
    #[n(1)]
    pub from: Option<Cow<'a, SkyNode>>,
}

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
pub struct FindNodesResponse {
    #[n(0)]
    pub sky_nodes: MaxSizedVec<SkyNode, 20>,
}

impl From<Vec<SkyNode>> for FindNodesResponse {
    fn from(sky_nodes: Vec<SkyNode>) -> Self {
        Self {
            sky_nodes: sky_nodes.into_iter().collect(),
        }
    }
}

#[derive(Clone)]
pub struct KadHandler {
    transport: quinn_transport::Transport,
}

impl From<quinn_transport::Transport> for KadHandler {
    fn from(transport: quinn_transport::Transport) -> Self {
        Self { transport }
    }
}

impl KadHandler {
    pub fn new(transport: quinn_transport::Transport) -> Self {
        Self { transport }
    }

    pub(crate) fn transport(&self) -> &quinn_transport::Transport {
        &self.transport
    }
}

impl kademlia::RequestHandler<SkyNode, SkyNode, 32> for KadHandler {
    async fn ping(&self, from: &SkyNode, node: &SkyNode) -> bool {
        if node.last_reached_at().elapsed() < Duration::from_secs(120) {
            return true;
        }
        let Ok(conn) = self.transport.connect(node).await else {
            trace!("failed to ping {node:?}");
            return false;
        };

        let Ok(_) = conn
            .query::<shared_schema::ping::Method, RootRequest>(shared_schema::ping::Request)
            .await
        else {
            trace!("failed to ping {node:?}");
            return false;
        };

        true
    }

    async fn find_node(
        &self,
        from: &SkyNode,
        to: &SkyNode,
        address: &kademlia::Id<32>,
    ) -> Vec<SkyNode> {
        let Ok(conn) = self.transport.connect(to).await else {
            return vec![];
        };

        let Ok(nodes) = conn
            .query::<FindNodesMethod, RootRequest>(FindNodesRequest {
                sky_id: unsafe {
                    // SAFETY: this kademlia handler is only used on SkyNodes, so the lookup operations are
                    // operating in SkyId space.
                    SkyId::from_kademlia_id_unchecked(address.clone())
                },
                from: Some(Cow::Borrowed(from)),
            })
            .await
        else {
            return vec![];
        };

        todo!()
    }
}

#[derive(Clone)]
pub struct FindNodesMethod<'a> {
    rpc_manager:
        kademlia::RpcManager<SkyNode, SkyNode, KadHandler, KadNodeCache<SkyNode, 32, 20>, 32, 20>,
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> FindNodesMethod<'a> {
    pub fn new(transport: &quinn_transport::Transport, me: SkyNode) -> Self {
        let rpc_manager = kademlia::RpcManager::new(
            KadHandler {
                transport: transport.clone(),
            },
            me.clone(),
            KadNodeCache::new(me),
        );
        Self {
            rpc_manager,
            _phantom: std::marker::PhantomData,
        }
    }

    fn node_lookup() {
        todo!()
    }
}

impl<'a> rpc::Method for FindNodesMethod<'a> {
    type Req = FindNodesRequest<'a>;

    type Res = FindNodesResponse;

    type Error = Infallible;
}

impl<'a> rpc::Call for FindNodesMethod<'a> {
    async fn call(&mut self, value: Self::Req) -> Result<FindNodesResponse, Infallible> {
        let sky_id: kademlia::Id<32> = value.sky_id.into();
        let owned = value.from.map(Cow::into_owned);
        let pinned: std::pin::Pin<Box<_>> = Box::pin(async {
            let out = self.rpc_manager.find_node(owned, &sky_id).await;
            Ok(out.into())
        });
        pinned.await
    }
}

impl From<ping::Request> for RootRequest<'_> {
    fn from(value: ping::Request) -> Self {
        Self::Ping(value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}

impl From<Infallible> for Error {
    fn from(_value: Infallible) -> Self {
        unreachable!()
    }
}

#[derive(Clone)]
pub struct RootHandler<'a> {
    find_nodes_handler: FindNodesMethod<'a>,
}

#[derive(Debug, thiserror::Error)]
pub enum RootHandlerConfigError {
    #[error("couldn't get public ip address")]
    NoPublicIp,
}

impl<'a> RootHandler<'a> {
    pub async fn new(tp: &quinn_transport::Transport) -> Result<Self, RootHandlerConfigError> {
        let pub_ip = get_public_ip()
            .await
            .ok_or(RootHandlerConfigError::NoPublicIp)?;
        let find_nodes_handler = FindNodesMethod::new(&tp, pub_ip.into());
        Ok(Self { find_nodes_handler })
    }
}

impl<'a> rpc::RootHandler<RootRequest<'a>> for RootHandler<'a> {
    type Error = Error;

    async fn handle<T: futures_io::AsyncWrite + Unpin + Sync + Send, TransportError>(
        &mut self,
        root: RootRequest<'a>,
        replier: rpc::Replier<'_, T>,
    ) -> Result<rpc::ReplyReceipt, rpc::ClientError<TransportError, Self::Error>> {
        match root {
            RootRequest::Ping(request) => ping::Method.reply(replier, request).await,
            RootRequest::FindNodes(request) => {
                self.find_nodes_handler.reply(replier, request).await
            }
        }
    }
}
#[derive(Clone)]
pub struct SkyServer {
    handler: RootHandler<'static>,
    transport: quinn_transport::Transport,
}

impl SkyServer {
    pub async fn new() -> Result<Self, ClientError<quinn_transport::Error, Error>> {
        let tp = quinn_transport::Transport::self_signed_server()
            .await
            .map_err(ClientError::Transport)?;
        let handler = RootHandler::new(&tp).await.unwrap();

        Ok(Self {
            handler,
            transport: tp,
        })
    }

    pub fn into_parts(self) -> (RootHandler<'static>, quinn_transport::Transport) {
        (self.handler, self.transport)
    }
    pub async fn add_nodes(&self, nodes: Vec<SkyNode>) {
        self.handler
            .find_nodes_handler
            .rpc_manager
            .add_nodes(nodes)
            .await;
    }
    pub async fn run(self) -> Result<(), ClientError<quinn_transport::Error, Error>> {
        let mut js: JoinSet<Result<(), ClientError<quinn_transport::Error, Error>>> =
            JoinSet::new();

        self.handler
            .find_nodes_handler
            .rpc_manager
            .join_network()
            .await;

        let (handler, tp) = self.into_parts();
        loop {
            let incoming_client: quinn_transport::Incoming =
                tp.accept().await.map_err(ClientError::Transport)?;
            let handler = handler.clone();
            let span =
                trace_span!("from client", client = %incoming_client.inner().remote_address());
            js.spawn( async move {

                let conn = rpc::Incoming::accept(incoming_client)
                    .await.map_err(ClientError::Transport)?;
                trace!("accepted client");

                if let Err(e) = conn.handle_client::<RootRequest, _>(handler).await {
                    match e {
                        ClientError::Transport(crate::quinn_transport::Error::Connection(
                            quinn::ConnectionError::ApplicationClosed(close),
                        )) if close.error_code == 0u32.into()
                            && close.reason.len() == 0 =>
                        {
                            tracing::debug!("normal client closure")
                        },
                        ClientError::Transport(crate::quinn_transport::Error::Connection(quinn::ConnectionError::TimedOut)) => {
                            tracing::warn!("connection to server timed out. Okay if the client still got all their responses")
                        }
                        e => Err(e).unwrap(),
                    }
                }
                Ok(())
                // Result<(), std::error::Error>::Ok(())
            }.instrument(span));
            while let Some(result) = js.try_join_next() {
                result.unwrap()?;
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use end_to_end_test::sim::machine::HasMachineId;
    use maxlen::MaxLen as _;
    use minicbor::CborLen as _;
    use rand::Rng;
    use shared_schema::{ping, sky_node::SkyId};
    use std::{
        borrow::Cow,
        net::{IpAddr, Ipv4Addr},
        time::Duration,
    };
    use tracing_subscriber::fmt::time::tokio_uptime;
    use tracing_subscriber::fmt::writer::MakeWriterExt;
    use tracing_subscriber::fmt::{MakeWriter, format::Format};

    use rpc::{Caller as _, Client as _, ClientError, Close, Incoming, Transport as _};
    use tokio::task::JoinSet;
    use tracing::{Instrument, Level, debug, span, trace, warn};

    use end_to_end_test::{
        Host, OsShim,
        net::ip,
        sim::{MachineRef, RNG, Sim},
    };

    use crate::{
        quinn_transport::Transport,
        request_handler::{FindNodesRequest, RootHandler, RootRequest, SkyServer},
    };

    #[test]
    fn test_root_request_max_len() {
        let ping_variant = RootRequest::Ping(ping::Request);
        let find_nodes_variant = RootRequest::FindNodes(FindNodesRequest {
            sky_id: SkyId::from(IpAddr::V4([169, 168, 0, 1].into())),
            from: Some(Cow::Owned(IpAddr::V4([169, 168, 0, 1].into()).into())),
        });

        let biggest_inst = RootRequest::biggest_instantiation();
        println!("{:?}", biggest_inst);
        println!("{:?}", biggest_inst.cbor_len(&mut ()));
        println!("{:?}", ping_variant.cbor_len(&mut ()));

        // get max_len
        let max_len = RootRequest::max_len();
        println!("{:?}", max_len);

        assert!(RootRequest::max_len() as u32 >= ping_variant.cbor_len(&mut ()) as u32);

        assert!(RootRequest::max_len() as u32 >= find_nodes_variant.cbor_len(&mut ()) as u32);
    }

    pub fn create_server() -> MachineRef<OsShim> {
        let server = OsShim::new(Host::new(move || async {
            Ok(SkyServer::new().await?.run().await?)
        }));
        server
    }

    pub fn create_ping_client(server_addr: std::net::IpAddr) -> MachineRef<OsShim> {
        OsShim::new(Host::new(move || {
            let span = span!(Level::DEBUG, "client");
            async move {
                let tp = Transport::client().await?;
                trace!("inited");
                let base_ms: f64 = 5_000.0;
                let max_ms: f64 = 30_000.0; // 30 second cap
                let mut last_delay_ms: f64 = base_ms;

                let conn = loop {
                    match tp.connect(&server_addr.into()).await {
                        Ok(c) => break c,
                        Err(crate::quinn_transport::Error::Connection(
                            quinn::ConnectionError::TimedOut,
                        )) => {
                            // Decorrelated jitter: min(cap, random(base, last_delay * 3))
                            let max_jitter: f64 = (last_delay_ms * 3.0).min(max_ms);
                            let sleep_ms = RNG.with(|rng| {
                                rng.borrow_mut()
                                    .random_range(base_ms as u64..max_jitter as u64)
                                    as f64
                            });
                            last_delay_ms = sleep_ms;
                            let sleep_duration =
                                tokio::time::Duration::from_millis(sleep_ms as u64);
                            trace!("sleeping for {:?}", sleep_duration);
                            tokio::time::sleep(sleep_duration).await;
                            continue;
                        }
                        Err(e) => Err(e)?,
                    }
                };
                let mut js: JoinSet<Result<(), Box<dyn std::error::Error>>> = JoinSet::new();

                for _ in 0..10 {
                    let conn_clone = conn.clone();
                    js.spawn_local(async move {
                        conn_clone
                            .query::<shared_schema::ping::Method, super::RootRequest>(
                                shared_schema::ping::Request,
                            )
                            .await
                            .unwrap();
                        Ok(())
                    });
                }

                while let Some(result) = js.join_next().await {
                    result?.unwrap();
                }

                conn.close().await;
                tp.close().await;

                trace!("got resp");
                Ok(())
            }
            .instrument(span)
        }))
    }

    #[test]
    pub fn ping() {
        let sim = Sim::new_with_config(end_to_end_test::sim::Config {
            // need to increase otherwise machine will end up stuck in WouldBlock loop
            nic_capacity: 1000,
            udp_capacity: 1000,
            ip_hop_capacity: 1000,
            tick_amount: Duration::from_millis(10),
            ..Default::default()
        });
        sim.enter_runtime(|| {
            // Create a `fmt` subscriber that uses our custom event format, and set it
            // as the default.
            tracing_subscriber::fmt()
                .pretty()
                .with_test_writer()
                .with_timer(tokio_uptime())
                .with_env_filter("sky=debug,end_to_end_test=debug,rpc=warn")
                .init();
            let net = Sim::add_machine(ip::Network::new());
            let server = create_server();

            let server_addr = server.get().borrow().connect_to_net(net);
            server.get().borrow().set_public_ip(server_addr);
            Sim::tick_machine(server).unwrap();
            let mut arr = vec![];
            for _ in 0..10000 {
                let client = create_ping_client(server_addr);
                let client_ip = client.get().borrow().connect_to_net(net);
                client.get().borrow().set_public_ip(client_ip);
                arr.push(client);
            }
            Sim::run_until_idle(|| arr.iter()).unwrap();
        })
    }
    #[test_log::test]
    pub fn test_kad() {}
}

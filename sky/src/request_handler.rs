mod find_nodes_method;
mod root_request;

pub use find_nodes_method::KadRpcManager;

use std::{convert::Infallible, net::IpAddr, time::Duration};

pub use find_nodes_method::{FindNodesMethod, FindNodesRequest, FindNodesResponse};
pub use root_request::RootRequest;

use rpc::Call;
use shared_schema::{SkyNode, ping};

use crate::{client::SkyOrEarth, quinn_transport};

impl<'a> From<FindNodesRequest> for RootRequest {
    fn from(value: FindNodesRequest) -> Self {
        Self::FindNodes(value)
    }
}

impl From<ping::Request> for RootRequest {
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
pub struct RootHandler {
    find_nodes_handler: FindNodesMethod,
    from: SkyOrEarth,
}

#[derive(Debug, thiserror::Error)]
pub enum RootHandlerConfigError {
    #[error("couldn't get public ip address")]
    NoPublicIp,
}

impl RootHandler {
    pub fn new(from: SkyOrEarth, kad_rpc_manager: &KadRpcManager) -> Self {
        let find_nodes_handler =
            FindNodesMethod::from_manager(&kad_rpc_manager, from.as_sky().cloned());
        Self {
            find_nodes_handler,
            from,
        }
    }

    pub fn local_node(&self) -> &SkyNode {
        self.find_nodes_handler.local_node()
    }

    pub async fn add_nodes(&self, nodes: impl IntoIterator<Item = SkyNode>) {
        self.find_nodes_handler.add_nodes(nodes).await
    }

    pub async fn refresh_stale_buckets(&self, duration: Duration) {
        self.find_nodes_handler
            .refresh_stale_buckets(&duration)
            .await
    }

    pub async fn join_network(&self) {
        self.find_nodes_handler.join_network().await
    }
}

impl rpc::RootHandler<RootRequest> for RootHandler {
    type Error = Error;

    async fn handle<T: futures_io::AsyncWrite + Unpin + Sync + Send, TransportError>(
        &mut self,
        root: RootRequest,
        replier: rpc::Replier<'_, T>,
    ) -> Result<rpc::ReplyReceipt, rpc::ClientError<TransportError, Self::Error>> {
        match root {
            RootRequest::Ping(request) => {
                self.find_nodes_handler.on_ping(self.from.clone()).await;

                ping::Method.reply(replier, request).await
            }
            RootRequest::FindNodes(request) => {
                self.find_nodes_handler.reply(replier, request).await
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use maxlen::MaxLen as _;
    use minicbor::CborLen as _;
    use rand::Rng;
    use shared_schema::{ping, sky_node::SkyId};
    use std::{net::IpAddr, time::Duration};
    use tracing_subscriber::fmt::time::tokio_uptime;

    use rpc::{Caller as _, Close, Transport as _};
    use tokio::task::JoinSet;
    use tracing::{Instrument, Level, span, trace};

    use dens::{
        OsMock,
        net::ip,
        sim::{MachineIntoRef as _, MachineRef, RNG, Sim},
    };

    use crate::{
        quinn_transport::Transport,
        request_handler::{FindNodesRequest, RootRequest},
        server::SkyServer,
    };

    #[test]
    fn test_root_request_max_len() {
        let ping_variant = RootRequest::Ping(ping::Request);
        let find_nodes_variant = RootRequest::FindNodes(FindNodesRequest {
            sky_id: SkyId::from(IpAddr::V4([169, 168, 0, 1].into())),
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

    pub fn create_server() -> MachineRef<OsMock> {
        let server =
            OsMock::new(move || async { Ok(SkyServer::new().await?.run().await??) }).into_ref();
        server
    }

    pub fn create_ping_client(server_addr: std::net::IpAddr) -> MachineRef<OsMock> {
        OsMock::new(move || {
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
        })
        .into_ref()
    }

    #[test]
    pub fn ping() {
        let sim = Sim::new_with_config(dens::sim::Config {
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
                .with_env_filter("sky=debug,dens=debug,rpc=warn")
                .init();
            let net = Sim::add_machine(ip::Network::new_private_class_c());
            let server = create_server();

            let server_addr = server.get().borrow().connect_to_net(net);
            Sim::tick_machine(server).unwrap();
            let mut arr = vec![];
            for _ in 0..4_000 {
                let client = create_ping_client(server_addr.0.into());
                let _client_ip = client.get().borrow().connect_to_net(net);
                arr.push(client);
            }
            Sim::run_until_idle(|| arr.iter()).unwrap();
        })
    }
}

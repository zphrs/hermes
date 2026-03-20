use rpc::Caller as _;
use std::time::{Duration, SystemTime};

use kademlia::{
    HasServerId, RpcManager,
    node_cache::{Cullable, NodeCache},
};
use rpc::Transport;
use shared_schema::{EarthNode, SkyNode, earth_node::EarthId, sky_node::SkyId};
use tracing::{debug, trace, warn};

use crate::{
    quinn_transport,
    request_handler::{FindNodesMethod, FindNodesRequest, KadHandler, RootRequest},
};
#[derive(Clone)]
struct WrappedEarthNode {
    inner: EarthNode,
}

impl From<EarthNode> for WrappedEarthNode {
    fn from(inner: EarthNode) -> Self {
        Self { inner }
    }
}

impl WrappedEarthNode {
    pub fn new(inner: EarthNode) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> EarthNode {
        self.inner
    }
}

impl HasServerId<SkyNode, 32> for WrappedEarthNode {
    fn server_id(&self) -> kademlia::Id<32> {
        let curr_time = get_system_time();
        let since_epoch = curr_time
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        self.inner.earth_id().to_sky_id(since_epoch).into()
    }
}

pub fn get_system_time() -> SystemTime {
    #[cfg(test)]
    let curr_time = {
        use end_to_end_test::{OsShim, sim::Sim};

        if Sim::is_in_machine_type::<OsShim>() {
            use end_to_end_test::Machine;

            let shim = Sim::get_current_machine::<OsShim>();
            let systime = shim.borrow().basic_machine().sys_time();
            debug!(?systime);
            systime
        } else {
            panic!()
        }
    };
    #[cfg(not(test))]
    let curr_time = {
        use std::time::SystemTime;
        SystemTime::now()
    };
    curr_time
}

pub struct KadClient {
    manager: RpcManager<WrappedEarthNode, SkyNode, KadHandler, Cache, 32, 20>,
}

impl KadClient {
    pub fn new(local: EarthNode, bootstraps: Vec<SkyNode>, tp: quinn_transport::Transport) -> Self {
        let handler: KadHandler = tp.into();
        let cache = Cache { bootstraps };
        Self {
            manager: RpcManager::new(handler, WrappedEarthNode { inner: local }, cache),
        }
    }

    pub async fn node_lookup(&self, near: EarthId) -> Vec<SkyNode> {
        let sky_id = near.to_sky_id(
            get_system_time()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap(),
        );
        debug!("nli for {:?} (sky equiv: {})", near, sky_id);
        self.manager.node_lookup(&sky_id.into()).await
    }
}

// for now simply store the bootstrap list, eventually should query
// for the bootstraps
struct Cache {
    bootstraps: Vec<SkyNode>,
}

impl Cache {
    pub fn new(bootstraps: Vec<SkyNode>) -> Self {
        Self { bootstraps }
    }
}

impl Cullable<WrappedEarthNode, SkyNode, 32> for Cache {
    type CullSet = ();

    async fn find_removal_candidates(
        &self,
        _nodes: impl Iterator<Item = SkyNode>,
        _handler: &impl kademlia::RequestHandler<WrappedEarthNode, SkyNode, 32>,
    ) -> Self::CullSet {
        warn!("actually return dead sky nodes here!");
        ()
    }

    fn remove_candidates(&mut self, _candidates: Self::CullSet) {
        warn!("actually return dead sky nodes here!");
        ()
    }
}

impl NodeCache<WrappedEarthNode, SkyNode, 32> for Cache {
    fn add_nodes(&mut self, _nodes: impl IntoIterator<Item = SkyNode>) {
        warn!("no-op: should eventually have this store nodes uniformly throughout the hash space")
    }

    fn nearby_nodes<'a>(&'a self, _address: &kademlia::Id<32>) -> impl Iterator<Item = &'a SkyNode>
    where
        SkyNode: 'a,
    {
        self.bootstraps.iter()
    }
}

impl kademlia::RequestHandler<WrappedEarthNode, SkyNode, 32> for KadHandler {
    async fn ping(&self, from: &WrappedEarthNode, node: &SkyNode) -> bool {
        if node.last_reached_at().elapsed() < Duration::from_secs(120) {
            return true;
        }
        let Ok(conn) = self.transport().connect(node).await else {
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
        _from: &WrappedEarthNode,
        to: &SkyNode,
        address: &kademlia::Id<32>,
    ) -> Vec<SkyNode> {
        let Ok(conn) = self.transport().connect(to).await else {
            return vec![];
        };

        let Ok(nodes) = conn
            .query::<FindNodesMethod, RootRequest>(FindNodesRequest {
                sky_id: unsafe {
                    // SAFETY: We're only querying for sky nodes using RpcManager.
                    // Node lookup requests from earth node space are converted before
                    // being sent into RpcManager.
                    SkyId::from_kademlia_id_unchecked(address.clone())
                },
                from: None,
            })
            .await
        else {
            return vec![];
        };

        nodes.sky_nodes.into_inner().into_iter().collect()
    }
}

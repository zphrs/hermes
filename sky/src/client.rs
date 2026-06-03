mod cache;
mod sky_or_earth;
pub use sky_or_earth::SkyOrEarth;
use std::time::Duration;

use rpc::Transport;
use shared_schema::{EarthNode, SkyNode, sky_node::SkyId};
use tracing::{debug, instrument, warn};

use crate::{
    api::find_nodes,
    entrypoint::{self, as_sky},
    get_system_time::get_system_time,
    quinn_transport,
};

pub struct SkyClient {
    bootstrap_nodes: Vec<SkyNode>,
}

struct Handler {
    transport: quinn_transport::Transport,
}

use rpc::client_conn::Wrapper;

use kademlia::traits::NodeStatus;
impl kademlia::RequestHandler<SkyOrEarth, 32> for Handler {
    fn node_status(&self, _from: &SkyOrEarth, node: &SkyOrEarth) -> NodeStatus {
        let Some(sky_node) = node.as_sky() else {
            warn!("earth lookup");
            // this is a client for the sky, earth nodes should not be here
            return NodeStatus::Bad;
        };
        if sky_node.last_reached_at().elapsed() < Duration::from_secs(60) {
            return NodeStatus::Good;
        }
        return NodeStatus::Unknown;
    }

    #[instrument(skip(self))]
    async fn ping(&self, from: &SkyOrEarth, node: &SkyOrEarth) -> bool {
        let Some(sky_node) = node.as_sky() else {
            warn!("earth lookup");
            return false; // this is a client for the sky, doesn't make sense to ping an earth node
        };
        if sky_node.last_reached_at().elapsed() < Duration::from_secs(60) {
            return true;
        }

        debug!("pinging");
        warn!("should reuse conns instead of making new conn for every ping req");
        let Ok(conn) = self.transport.connect(sky_node).await else {
            return false;
        };

        let login: Wrapper<entrypoint::Method, _> = Wrapper::new(conn);
        let Ok(v) = login.query::<as_sky::Method>(Default::default()).await else {
            return false;
        };

        let sky_root: Wrapper<_, _> = v
            .map_res(|res| match res {
                as_sky::Response::Ok(method_wrapper) => method_wrapper,
                as_sky::Response::Invalid(_method_wrapper) => unimplemented!(),
            })
            .into();

        let reached_node = sky_root
            .query_loopback::<shared_schema::ping::Method>(shared_schema::ping::Request)
            .await
            .is_ok();
        if reached_node {
            sky_node.reset_last_reached_at();
        }
        reached_node
    }

    #[instrument(skip(self))]
    async fn find_node(
        &self,
        from: &SkyOrEarth,
        to: &SkyOrEarth,
        address: &kademlia::Id<32>,
    ) -> Vec<SkyOrEarth> {
        debug!("finding node");
        let Some(to) = to.as_sky() else {
            return vec![]; // this is a client for the sky, doesn't make sense to find through earth nodes
        };
        warn!("should reuse conns instead of making new conn for every ping req");
        let Ok(conn) = self.transport.connect(to).await else {
            return vec![];
        };

        let login: Wrapper<entrypoint::Method, _> = Wrapper::new(conn);
        let Ok(v) = login.query::<as_sky::Method>(Default::default()).await else {
            return vec![];
        };

        let sky_root: Wrapper<_, _> = v
            .map_res(|res| match res {
                as_sky::Response::Ok(method_wrapper) => method_wrapper,
                as_sky::Response::Invalid(_method_wrapper) => unimplemented!(),
            })
            .into();

        let Ok(v) = sky_root
            .query_loopback::<find_nodes::Method>(find_nodes::Request {
                sky_id: unsafe { SkyId::from_kademlia_id_unchecked(address.clone()) },
            })
            .await
        else {
            return vec![];
        };
        v.into_inner().into_iter().map(SkyOrEarth::sky).collect()
    }
}

impl SkyClient {
    pub fn new(bootstrap_nodes: Vec<SkyNode>) -> Self {
        Self { bootstrap_nodes }
    }

    /// searches net for the node closest to id
    pub async fn node_lookup(
        &self,
        tp: quinn_transport::Transport,
        from: EarthNode,
        id: SkyId,
    ) -> Vec<SkyNode> {
        debug!("looking up node");
        let table = kademlia::RpcManager::<SkyOrEarth, _, 32, 1>::new(
            Handler { transport: tp },
            (from, get_system_time()).into(),
        );
        table
            .add_nodes(self.bootstrap_nodes.iter().cloned().map(SkyOrEarth::sky))
            .await;

        let out = table
            .node_lookup(&id.into())
            .await
            .into_iter()
            .map(|v| v.into_sky())
            .filter_map(|v| v)
            .collect();

        warn!(
            "should add back to the bootstrap_nodes list at some point based on the kademlia dump of nodes"
        );

        out
    }
}

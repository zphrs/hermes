mod sky_or_earth;

pub use sky_or_earth::SkyOrEarth;
use std::{borrow::Cow, time::Duration};

use rpc::{Caller, Transport};
use shared_schema::{EarthNode, SkyNode, sky_node::SkyId};
use tracing::{debug, instrument, warn};

use crate::{
    get_system_time::get_system_time,
    quinn_transport,
    request_handler::{self, FindNodesRequest, RootRequest},
};

pub struct SkyClient {
    bootstrap_nodes: Vec<SkyNode>,
}

struct Handler {
    transport: quinn_transport::Transport,
}

impl kademlia::RequestHandler<SkyOrEarth, 32> for Handler {
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
        let Ok(conn) = self.transport.connect(sky_node).await else {
            return false;
        };

        if let Ok(_v) = conn
            .query::<shared_schema::ping::Method, crate::request_handler::RootRequest>(
                shared_schema::ping::Request,
            )
            .await
        {
            sky_node.reset_last_reached_at();
            return true;
        }
        return false;
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

        let Ok(conn) = self.transport.connect(to).await else {
            return vec![]; // unreachable
        };

        let Ok(v): Result<
            request_handler::FindNodesResponse,
            rpc::CallerError<quinn_transport::Error>,
        > = conn
            .query::<request_handler::FindNodesMethod, RootRequest>(FindNodesRequest {
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

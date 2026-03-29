use std::{
    borrow::Cow,
    time::{Duration, UNIX_EPOCH},
};

use kademlia::HasId;
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkyOrEarth {
    Sky(SkyNode),
    Earth(EarthNode),
}

#[derive(Clone, Debug, Eq)]
pub struct SkyOrEarthNode {
    id: kademlia::Id<32>,
    inner: SkyOrEarth,
}

impl From<SkyNode> for SkyOrEarthNode {
    fn from(value: SkyNode) -> Self {
        Self::sky(value)
    }
}

impl From<EarthNode> for SkyOrEarthNode {
    fn from(value: EarthNode) -> Self {
        Self::earth(value)
    }
}

impl PartialEq for SkyOrEarthNode {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl SkyOrEarthNode {
    pub fn sky(sky: SkyNode) -> Self {
        Self {
            id: sky.id().clone(),
            inner: SkyOrEarth::Sky(sky),
        }
    }
    pub fn earth(earth: EarthNode) -> Self {
        let id = earth
            .earth_id()
            .to_sky_id(get_system_time().duration_since(UNIX_EPOCH).unwrap());
        Self {
            id: id.into(),
            inner: SkyOrEarth::Earth(earth),
        }
    }

    pub fn as_sky(&self) -> Option<&SkyNode> {
        match &self.inner {
            SkyOrEarth::Sky(sky_node) => Some(sky_node),
            _ => None,
        }
    }

    pub fn as_earth(&self) -> Option<&EarthNode> {
        match &self.inner {
            SkyOrEarth::Earth(earth) => Some(earth),
            _ => None,
        }
    }

    pub fn into_sky(self) -> Option<SkyNode> {
        match self.inner {
            SkyOrEarth::Sky(sky) => Some(sky),
            _ => None,
        }
    }
}

impl HasId<32> for SkyOrEarthNode {
    fn id(&self) -> &kademlia::Id<32> {
        &self.id
    }
}

impl kademlia::RequestHandler<SkyOrEarthNode, 32> for Handler {
    #[instrument(skip(self))]
    async fn ping(&self, _from: &SkyOrEarthNode, node: &SkyOrEarthNode) -> bool {
        debug!("pinging");
        let Some(sky_node) = node.as_sky() else {
            warn!("earth lookup");
            return false; // this is a client for the sky, doesn't make sense to ping an earth node
        };
        let Ok(conn) = self.transport.connect(sky_node).await else {
            return false;
        };

        if let Ok(_v) = conn
            .query::<shared_schema::ping::Method, crate::request_handler::RootRequest>(
                shared_schema::ping::Request,
            )
            .await
        {
            return true;
        }
        return false;
    }
    #[instrument(skip(self))]
    async fn find_node(
        &self,
        from: &SkyOrEarthNode,
        to: &SkyOrEarthNode,
        address: &kademlia::Id<32>,
    ) -> Vec<SkyOrEarthNode> {
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
                from: from.as_sky().map(Cow::Borrowed),
            })
            .await
        else {
            return vec![];
        };
        v.into_inner()
            .into_iter()
            .map(SkyOrEarthNode::sky)
            .collect()
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
        from: SkyOrEarthNode,
        id: SkyId,
    ) -> Vec<SkyNode> {
        debug!("looking up node");
        let table =
            kademlia::RpcManager::<SkyOrEarthNode, _, 32, 20>::new(Handler { transport: tp }, from);
        table
            .add_nodes(
                self.bootstrap_nodes
                    .iter()
                    .cloned()
                    .map(SkyOrEarthNode::sky),
            )
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

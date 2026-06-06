pub mod join;
pub mod nearby;
pub mod renew;

use std::{
    collections::BTreeMap,
    convert::Infallible,
    net::IpAddr,
    sync::{Arc, RwLock},
};

use max_sized_vec::MaxSizedVec;
use maxlen::MaxLen;
use shared_schema::{EarthNode, earth_node::EarthId};
use tokio::time::Instant;

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Request {
    /// to initially join the network
    #[n(0)]
    Join(#[n(0)] join::Request),
    /// to renew the join after 20 seconds
    #[n(1)]
    Renew(#[n(0)] renew::Request),
    /// to renew the join after 20 seconds
    #[n(2)]
    Nearby(#[n(0)] nearby::Request),
}

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct JoinReceipt();

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Response {
    #[n(0)]
    Join(#[n(0)] join::Response),
    #[n(1)]
    Renew(#[n(0)] renew::Response),
    #[n(2)]
    Nearby(#[n(0)] nearby::Response),
}

struct OnlineNode {
    remote: EarthNode,
    connection_candidates: Candidates,
    last_seen: Instant,
}

#[derive(Clone, Default)]
pub struct OnlineNodes(Arc<RwLock<BTreeMap<EarthId, OnlineNode>>>);

impl OnlineNodes {
    pub fn new() -> Self {
        Default::default()
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, BTreeMap<EarthId, OnlineNode>> {
        self.0.read().unwrap()
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, BTreeMap<EarthId, OnlineNode>> {
        self.0.write().unwrap()
    }
}

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
#[cbor(transparent)]
pub struct Candidates(#[n(0)] MaxSizedVec<IpAddr, 8>);

#[derive(Clone)]
pub struct Method {
    online_nodes: OnlineNodes,
    remote: EarthNode,
}
impl Method {
    pub(crate) fn from_online_nodes(online_nodes: OnlineNodes, remote: EarthNode) -> Self {
        Self {
            online_nodes,
            remote,
        }
    }
}

impl rpc::Method for Method {
    type Req = Request;
    type Res = Response;

    type Error = Infallible;
}

impl rpc::Call for Method {
    async fn call<T: futures_io::AsyncWrite + Unpin + Send + Sync, TransportError>(
        &mut self,
        replier: rpc::Replier<'_, T, Self>,
        value: Self::Req,
    ) -> Result<rpc::ReplyReceipt<Self::Res>, rpc::ClientError<TransportError, Self::Error>> {
        Ok(match value {
            Request::Join(candidates) => replier
                .change_method(&candidates)
                .reply_with(
                    &mut join::Method {
                        remote: &self.remote,
                        map: &self.online_nodes,
                    },
                    candidates,
                )
                .await?
                .map(Response::Join),
            Request::Renew(join_receipt) => replier
                .change_method(&join_receipt)
                .reply_with(
                    &mut renew::Method {
                        remote: &self.remote,
                        map: &self.online_nodes,
                    },
                    join_receipt,
                )
                .await?
                .map(Response::Renew),
            Request::Nearby(earth_id) => replier
                .change_method(&earth_id)
                .reply_with(
                    &mut nearby::Method {
                        map: &self.online_nodes,
                    },
                    earth_id,
                )
                .await?
                .map(Response::Nearby),
        })
    }
}

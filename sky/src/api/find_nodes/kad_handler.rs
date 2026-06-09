use std::sync::Arc;
use std::{convert::Infallible, time::Duration};

use crate::api::sky_root;
use crate::client::{Cache, cache};
use crate::entrypoint::as_sky;

use rpc::client_conn::Wrapper;

use rpc::ClientError;
use shared_schema::{SkyNode, sky_node::SkyId};
use tokio::sync::RwLock;
use tracing::{debug, trace, warn};

use crate::quinn_transport;

#[derive(Clone)]
pub struct KadHandler {
    cache: Arc<RwLock<Cache<sky_root::Method, as_sky::Method>>>,
}

impl From<quinn_transport::Transport> for KadHandler {
    fn from(transport: quinn_transport::Transport) -> Self {
        Self::new(transport)
    }
}

impl KadHandler {
    pub fn new(transport: quinn_transport::Transport) -> Self {
        Self {
            cache: Arc::new(RwLock::new(Cache::new(transport, as_sky::Request::new()))),
        }
    }

    /// Executes an async operation with exponential backoff retry logic.
    ///
    /// # Parameters
    /// - `max_attempts`: Maximum number of retry attempts
    /// - `operation`: The async function to execute
    /// - `default_value`: The value to return if all attempts fail
    async fn retry_with_backoff<F, Fut, T>(
        max_attempts: usize,
        mut operation: F,
        default_value: T,
    ) -> T
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, ClientError<quinn_transport::Error, Infallible>>>,
    {
        for attempt in 0..max_attempts {
            // Sleep before retries (but not before the first attempt)
            if attempt > 0 {
                let min_delay = 4 * 2u64.pow((attempt - 1) as u32);
                let max_delay = min_delay * 2;
                let delay_secs = rand::random_range(min_delay..max_delay);
                tokio::time::sleep(Duration::from_secs(delay_secs)).await;
            }

            match operation().await {
                Ok(value) => {
                    debug!("found nodes");
                    return value;
                }
                Err(e) if attempt < max_attempts - 1 => {
                    debug!("{e} error, trying again: ({e:?})");
                }
                Err(e) => {
                    warn!("{e} error ({e:?}), returning empty");
                    break;
                }
            }
        }

        default_value
    }

    async fn try_ping(
        &self,
        _from: impl Into<shared_schema::Node>,
        node: &SkyNode,
    ) -> Result<(), ClientError<quinn_transport::Error, Infallible>> {
        if node.last_reached_at().elapsed() < Duration::from_secs(120) {
            trace!("returning early because we've heard from them recently");
            return Ok(());
        }
        let sky_root = self.get_sky_root(node).await?;

        sky_root
            .query_loopback::<shared_schema::ping::Method>(shared_schema::ping::Request)
            .await
            .map_err(ClientError::from_caller)?;

        node.reset_last_reached_at();
        Ok(())
    }

    async fn get_sky_root(
        &self,
        remote: &SkyNode,
    ) -> Result<
        Arc<Wrapper<sky_root::Method, quinn_transport::Caller>>,
        ClientError<quinn_transport::Error, Infallible>,
    > {
        let mut cache = self.cache.read().await;

        loop {
            match cache
                .try_connect(remote, &mut |res| match res {
                    as_sky::Response::Ok(sky_root) => Some(sky_root),
                    as_sky::Response::Invalid(_method_wrapper) => None,
                })
                .await
            {
                Ok(v) => break Ok(v),
                Err(cache::ConnectError::MissingNodeEntry(_)) => {
                    drop(cache);
                    self.cache.write().await.insert_node_entry(remote);
                    cache = self.cache.read().await;
                }
                Err(cache::ConnectError::Caller(e)) => {
                    break Err(ClientError::from_caller(e));
                }
                Err(cache::ConnectError::LoginFailed) => {
                    todo!("handle a login failure for sky-to-sky")
                }
            }
        }
    }
    #[tracing::instrument(skip(self))]
    async fn try_find_node(
        &self,
        _from: &SkyNode,
        to: &SkyNode,
        address: &kademlia::Id<32>,
    ) -> Result<Vec<SkyNode>, ClientError<quinn_transport::Error, Infallible>> {
        let sky_root = self.get_sky_root(to).await?;

        let nodes = sky_root
            .query_loopback::<super::Method>(super::Request {
                sky_id:
                // SAFETY: this kademlia handler is only used on SkyNodes, so the lookup operations are
                // operating in SkyId space.
                unsafe {
                    SkyId::from_kademlia_id_unchecked(address.clone())
                }
            })
            .await
            .map_err(ClientError::from_caller)?;

        Ok(nodes.into())
    }
}

impl kademlia::RequestHandler<SkyNode, 32> for KadHandler {
    fn node_status(&self, from: &SkyNode, node: &SkyNode) -> kademlia::traits::NodeStatus {
        // throw away in the def so auto generated impls and docs will not have
        // underscores in front of the parameters
        let _ = (from, node);
        if node.last_reached_at().elapsed() < Duration::from_secs(120) {
            kademlia::traits::NodeStatus::Good
        } else {
            kademlia::traits::NodeStatus::Unknown
        }
    }
    #[tracing::instrument(skip(self))]
    async fn ping(&self, from: &SkyNode, node: &SkyNode) -> bool {
        trace!("handling ping");
        Self::retry_with_backoff(
            2,
            || async { self.try_ping(from.clone(), node).await.map(|_| true) },
            false,
        )
        .await
    }
    #[tracing::instrument(skip(self))]
    async fn find_node(
        &self,
        from: &SkyNode,
        to: &SkyNode,
        address: &kademlia::Id<32>,
    ) -> Vec<SkyNode> {
        trace!("finding node");
        Self::retry_with_backoff(3, || self.try_find_node(from, to, address), vec![]).await
    }
}

use std::{borrow::Cow, convert::Infallible, time::Duration};

use super::{FindNodesMethod, FindNodesRequest};
use crate::request_handler::RootRequest;

use rpc::{Caller, ClientError, Transport};
use shared_schema::{SkyNode, sky_node::SkyId};
use tracing::{debug, trace, warn};

use crate::quinn_transport;

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

    /// Executes an async operation with exponential backoff retry logic.
    ///
    /// # Parameters
    /// - `max_attempts`: Maximum number of retry attempts
    /// - `operation`: The async function to execute
    /// - `default_value`: The value to return if all attempts fail
    async fn retry_with_backoff<F, Fut, T>(
        &self,
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
        node: &SkyNode,
    ) -> Result<(), ClientError<quinn_transport::Error, Infallible>> {
        if node.last_reached_at().elapsed() < Duration::from_secs(120) {
            trace!("returning early because we've heard from them recently");
            return Ok(());
        }

        let conn = self
            .transport
            .connect(node)
            .await
            .map_err(ClientError::Transport)?;

        conn.query::<shared_schema::ping::Method, RootRequest>(shared_schema::ping::Request)
            .await
            .map_err(ClientError::from_caller)?;
        node.reset_last_reached_at();
        Ok(())
    }
    #[tracing::instrument(skip(self))]
    async fn try_find_node(
        &self,
        from: &SkyNode,
        to: &SkyNode,
        address: &kademlia::Id<32>,
    ) -> Result<Vec<SkyNode>, ClientError<quinn_transport::Error, Infallible>> {
        let conn = self
            .transport
            .connect(to)
            .await
            .map_err(ClientError::Transport)?;

        let nodes = conn
            .query::<FindNodesMethod, RootRequest>(FindNodesRequest {
                sky_id:
                // SAFETY: this kademlia handler is only used on SkyNodes, so the lookup operations are
                // operating in SkyId space.
                unsafe {
                    SkyId::from_kademlia_id_unchecked(address.clone())
                },
                from: Some(Cow::Borrowed(from)),
            })
            .await
            .map_err(ClientError::from_caller)?;

        Ok(nodes.into())
    }
}

impl kademlia::RequestHandler<SkyNode, 32> for KadHandler {
    #[tracing::instrument(skip(self))]
    async fn ping(&self, from: &SkyNode, node: &SkyNode) -> bool {
        trace!("handling ping");
        self.retry_with_backoff(
            2,
            || async { self.try_ping(node).await.map(|_| true) },
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
        self.retry_with_backoff(3, || self.try_find_node(from, to, address), vec![])
            .await
    }
}

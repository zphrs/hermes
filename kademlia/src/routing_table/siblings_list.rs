use std::collections::HashSet;
use std::fmt::Debug;

use std::cmp::min;

use futures::StreamExt as _;
use futures::stream::FuturesUnordered;
use tokio::sync::Semaphore;
use tracing::warn;

use crate::routing_table::NEARBY_NODES_MULTIP;
use crate::{BUCKET_SIZE, Distance, DistancePair, HasId, Id, RequestHandler};

pub struct SiblingsList<Node: HasId<ID_LEN>, const ID_LEN: usize> {
    inner: Vec<DistancePair<Node, ID_LEN>>,
    ping: Semaphore,
}

impl<Node: HasId<ID_LEN> + Debug, const ID_LEN: usize> Debug for SiblingsList<Node, ID_LEN> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SiblingsList").field(&self.inner).finish()
    }
}

impl<Node: HasId<ID_LEN> + Clone, const ID_LEN: usize> Clone for SiblingsList<Node, ID_LEN> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            ping: Semaphore::new(1),
        }
    }
}

impl<Node: HasId<ID_LEN>, const ID_LEN: usize> Default for SiblingsList<Node, ID_LEN> {
    fn default() -> Self {
        Self {
            inner: Vec::with_capacity(NEARBY_NODES_MULTIP * BUCKET_SIZE + 1),
            ping: Semaphore::new(1),
        }
    }
}

impl<Node: Eq + Debug + HasId<ID_LEN>, const ID_LEN: usize> SiblingsList<Node, ID_LEN> {
    pub fn maybe_add_nodes<DP, Iter: IntoIterator<Item = DP>>(
        &mut self,
        pairs: Iter,
    ) -> impl IntoIterator<Item = DistancePair<Node, ID_LEN>>
    where
        DistancePair<Node, ID_LEN>: From<DP>,
    {
        let last_in_list: &Distance<_> = &self
            .inner
            .get(NEARBY_NODES_MULTIP * BUCKET_SIZE)
            .map(|p| p.distance().clone())
            .unwrap_or(Distance::MAX);
        let iter = pairs
            .into_iter()
            .map(DistancePair::from)
            .filter(|v| v.distance() < last_in_list);

        // splice inserts at the beginning of the vec
        self.inner.splice(0..0, iter);
        // sort is stable, which means that the newly inserted items will be
        // first in the list in the case there's an equal DistancePair already
        // in the list
        self.inner.sort();
        // `dedup` removes the second and subsequent duplicate items. In this
        // case, will keep the newly inserted node and remove all duplicate
        // nodes after the newly inserted one
        self.inner.dedup();
        self.inner
            .drain(min(NEARBY_NODES_MULTIP * BUCKET_SIZE, self.inner.len())..self.inner.len())
    }
    /// Only will handle one at a time, avoids having multiple unreachable
    /// ping requests occurring at once
    pub async fn get_unreachable_nodes(
        &self,
        local_node: &Node,
        handler: &impl RequestHandler<Node, ID_LEN>,
    ) -> HashSet<Id<ID_LEN>> {
        let mut to_remove_set = HashSet::new();
        warn!("move semaphore up to include the remove step as well");
        let _semaphore_permit = self.ping.acquire().await.unwrap();
        {
            let to_remove = FuturesUnordered::from_iter(self.inner.iter().map(|pair| async move {
                (!handler.ping(local_node, pair.node()).await).then_some(pair.node().id().clone())
            }));

            // once all have responded, continue.
            let mut chunks = to_remove.ready_chunks(BUCKET_SIZE);
            let mut total_pinged = 0;
            while let Some(chunk) = chunks.next().await {
                total_pinged += chunk.len();
                to_remove_set.extend(chunk.into_iter().flatten());
                if total_pinged >= self.inner.len() {
                    break;
                }
            }
        }
        to_remove_set
    }

    pub fn remove_nodes(&mut self, to_remove_set: HashSet<Id<ID_LEN>>) {
        self.inner = self
            .inner
            .drain(..)
            .filter(|pair| !to_remove_set.contains(pair.node().id()))
            .collect();
    }

    pub fn iter(&self) -> std::slice::Iter<'_, DistancePair<Node, ID_LEN>> {
        self.inner.iter()
    }

    pub fn nearest(
        &self,
        dist: &Distance<ID_LEN>,
        count: usize,
    ) -> std::slice::Iter<'_, DistancePair<Node, ID_LEN>> {
        let nearest_index = self
            .inner
            .binary_search_by_key(&dist, |pair| pair.distance())
            .unwrap_or_else(|err| err);

        let start_of_range = nearest_index.saturating_sub(count / 2);
        let end_of_range = min(nearest_index + count / 2, self.inner.len());
        self.inner[start_of_range..end_of_range].iter()
    }

    pub fn node_mut(
        &mut self,
        pair: &DistancePair<Node, ID_LEN>,
    ) -> Option<&mut DistancePair<Node, ID_LEN>> {
        self.inner
            .binary_search(pair)
            .ok()
            .map(|ind| &mut self.inner[ind])
    }
}

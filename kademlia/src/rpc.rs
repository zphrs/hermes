use std::{cmp::min, collections::HashSet, fmt::Debug, ops::Deref, sync::Arc, time::Duration};

use futures::{prelude::*, stream::FuturesUnordered};
// sync is runtime agnostic;
// see https://docs.rs/tokio/latest/tokio/sync/index.html#runtime-compatibility
use tokio::sync::{RwLock, RwLockWriteGuard};
use tracing::{Instrument, instrument, trace, trace_span};

use crate::{Distance, DistancePair, HasId, Id, RequestHandler, RoutingTable};

const ALPHA: usize = 3;

/// key is assumed to be an Id<ID_LEN>
#[derive(Clone)]
pub struct RpcManager<
    Node: HasId<ID_LEN>,
    Handler: RequestHandler<Node, ID_LEN>,
    const ID_LEN: usize,
    const BUCKET_SIZE: usize,
> {
    handler: Handler,
    pub(crate) routing_table: Arc<RwLock<RoutingTable<Node, ID_LEN, BUCKET_SIZE>>>,
    local_node: Node,
}

impl<
    Node: Eq + HasId<ID_LEN> + Clone + Debug,
    Handler: RequestHandler<Node, ID_LEN>,
    const ID_LEN: usize,
    const BUCKET_SIZE: usize,
> RpcManager<Node, Handler, ID_LEN, BUCKET_SIZE>
{
    pub fn new(handler: Handler, local_node: Node) -> Self {
        let routing_table = RoutingTable::new();

        Self {
            handler,
            routing_table: Arc::new(RwLock::new(routing_table)),
            local_node,
        }
    }

    fn get_shifted_target_id(&self, shift_by: usize) -> Id<ID_LEN> {
        let shifted_one: Distance<ID_LEN> = Distance::ONE << shift_by;

        let next_bucket_addr = &shifted_one;
        next_bucket_addr ^ self.local_addr()
    }
    /// refreshes all buckets which haven't been looked up within the past
    /// [duration](Duration).
    pub async fn refresh_stale_buckets(&self, duration: &Duration) {
        let dist_before = self.dist_to_closest_neighbor().await;
        self.node_lookup(self.local_node.id()).await;
        let dist_after = self.dist_to_closest_neighbor().await;

        if dist_after < dist_before {
            self.refresh_buckets_between(&dist_before, &dist_after)
                .await;
        }

        let futures: FuturesUnordered<_> = self
            .routing_table
            .read()
            .await
            .get_stale_buckets(duration)
            .map(|lz_count| self.refresh_bucket(lz_count))
            .collect();

        futures.count().await;
    }
    /// Should be scheduled to run about every hour for any bucket which hasn't
    /// been touched recently.
    async fn refresh_bucket(&self, lz_count: usize) {
        let shift_by = ID_LEN * 8 - lz_count;
        let target_id = self.get_shifted_target_id(shift_by);
        trace!("refreshing bucket");
        self.node_lookup(&target_id).await;
    }
    // 2.3: Refreshing means picking a random ID (just gonna do halfway) in the
    // bucket's range and performing a node search for that ID.
    // Run internally when joining network
    async fn refresh_buckets_between(&self, left: &Distance<ID_LEN>, right: &Distance<ID_LEN>) {
        // perform node_lookup on the bucket directly after the input distance.
        // could also simply do a left shift instead
        let lz_count = left.leading_zeros();
        let shift_by = ID_LEN * 8 - right.leading_zeros() - lz_count;
        let lookups = FuturesUnordered::new();
        trace!("refreshing {} buckets", lz_count);
        for shift_by in shift_by..(shift_by + lz_count) {
            // reverse engineer distance to id
            let target_id = self.get_shifted_target_id(shift_by);
            lookups.push(async move {
                self.node_lookup(&target_id).await;
            });
        }
        // await all refreshes
        let _ = lookups
            .count()
            // .timeout(futures_time::time::Duration::from_secs(10))
            .await;
    }
    async fn refresh_buckets_after(&self, after: &Distance<ID_LEN>) {
        self.refresh_buckets_between(after, &Distance::MAX).await
    }
    // 2.3: To join the network, a node u must have a contact to an already
    // participating node w. u inserts w into the appropriate k-bucket. u then
    // performs a node lookup for its own node ID. Finally, u refreshes all
    // k-buckets further away than its closest neighbor. During the refreshes, u
    // both populates its own k-buckets and inserts itself into other nodes'
    // k-buckets as necessary.
    #[instrument(skip(self))]
    pub async fn join_network(&self) {
        self.node_lookup(self.local_node.id()).await;
        let closest_dist = self.dist_to_closest_neighbor().await;
        if closest_dist == Distance::MAX {
            return;
        }

        self.refresh_buckets_after(&closest_dist).await;
    }

    async fn dist_to_closest_neighbor(&self) -> Distance<ID_LEN> {
        let routing_table = self.routing_table.read().await;
        routing_table
            .find_node(Distance::ZERO)
            .min()
            .map(|v| v.distance().clone())
            .unwrap_or(Distance::MAX)
            .clone()
    }

    fn local_addr(&self) -> &Id<ID_LEN> {
        self.local_node.id()
    }

    pub async fn add_node(&self, node: Node) {
        self.add_nodes([node]).await
    }

    async fn remove_and_insert_into_tree(
        &self,
        insertion_candidates: Vec<Option<Id<ID_LEN>>>,
        mut pairs: Vec<DistancePair<Node, ID_LEN>>,
        lock: &mut RwLockWriteGuard<'_, RoutingTable<Node, ID_LEN, BUCKET_SIZE>>,
    ) {
        let mut leaf = lock.get_leaf_mut(pairs[0].distance());
        let insertion_spots = insertion_candidates.len();
        let removals: HashSet<_> = insertion_candidates
            .iter()
            .filter_map(|v| v.as_ref())
            .collect();
        leaf.remove_where(|pair| removals.contains(&pair.node().id()));
        // since we either made room or returned early, we can safely
        // assume that the leaf likely has room when doing this insertion.
        for pair in pairs.drain(..min(insertion_spots, pairs.len())) {
            let _ = leaf.try_insert(pair);
        }
    }

    fn insert_into_tree(
        &self,
        pair: DistancePair<Node, ID_LEN>,
        lock: &mut RwLockWriteGuard<'_, RoutingTable<Node, ID_LEN, BUCKET_SIZE>>,
    ) -> bool {
        let mut leaf = lock.get_leaf_mut(pair.distance());
        // if full just skip pings
        leaf.try_insert(pair).is_ok()
    }

    async fn get_insertion_candidates(
        &self,
        pair: &DistancePair<Node, ID_LEN>,
    ) -> Vec<Option<Id<ID_LEN>>> {
        let routing_table = self.routing_table.read().await;
        let leaf = routing_table.get_leaf(pair.distance());
        let leaf_len = leaf.len();

        if leaf.is_full() {
            let mut failed_to_ping: Vec<Option<Id<_>>> = vec![];
            {
                let unordered_futures = FuturesUnordered::new();
                for pair in leaf.iter() {
                    unordered_futures.push(async move {
                        (
                            self.handler.ping(&self.local_node, pair.node()).await,
                            pair.node().id(),
                        )
                    });
                }
                let mut unordered_futures_chunks = unordered_futures.ready_chunks(leaf_len);
                while let Some(finished_pings) = unordered_futures_chunks.next().await {
                    for (ping_succeeded, id) in finished_pings {
                        if !ping_succeeded {
                            failed_to_ping.push(Some(id.clone()));
                        }
                    }
                    if !failed_to_ping.is_empty() {
                        // we've freed up at least some nodes
                        break;
                    }
                }
            }
            if failed_to_ping.is_empty() {
                // no room was freed up, all nodes in bucket were online
                return vec![];
            }
            failed_to_ping
        } else {
            vec![None; BUCKET_SIZE - leaf.len()]
        }
    }

    fn maybe_add_nodes_to_siblings_list<NodeIter: IntoIterator<Item = Node>>(
        &self,
        nodes: NodeIter,
        table_lock: &mut RwLockWriteGuard<'_, RoutingTable<Node, ID_LEN, BUCKET_SIZE>>,
    ) -> impl IntoIterator<Item = DistancePair<Node, ID_LEN>> {
        let local_addr = self.local_addr();
        let pairs = nodes.into_iter().map(move |node| (node, local_addr));
        table_lock.maybe_add_nodes_to_siblings_list(pairs)
    }

    /// pipelines the three stages for the nodes which allows all to complete
    /// their write, read, write stages in synchronicity when inserting.
    #[instrument(skip_all)]
    pub async fn add_nodes(&self, nodes: impl IntoIterator<Item = Node>) {
        let mut write_lock = self.routing_table.write().await;
        // first remove unreachable siblings list nodes
        write_lock
            .remove_unreachable_siblings_list_nodes(&self.local_node, &self.handler)
            .await;
        let nodes: Vec<_> = nodes
            .into_iter()
            .filter(|v| v.id() != self.local_addr())
            .filter(|n| {
                let dist_pair: DistancePair<Node, ID_LEN> = (n.clone(), self.local_addr()).into();
                let leaf = write_lock.get_leaf(dist_pair.distance());
                !leaf.contains(&dist_pair)
            })
            .collect();

        // now if there are any leftover, they are all alive nodes that
        // overflowed the siblings list

        let leftover: Vec<_> = self
            .maybe_add_nodes_to_siblings_list(nodes, &mut write_lock)
            .into_iter()
            .collect();

        drop(write_lock);
        // collect pairs into buckets
        let mut buckets: Vec<Vec<DistancePair<Node, ID_LEN>>> =
            vec![Vec::with_capacity(BUCKET_SIZE); Id::<ID_LEN>::BITS];

        for pair in leftover {
            let idx = Id::<ID_LEN>::BITS - 1 - pair.distance().leading_zeros();
            if buckets[idx].len() < BUCKET_SIZE {
                buckets[idx].push(pair);
            }
        }

        let unordered = FuturesUnordered::new();
        for bucket in buckets.drain(..).filter(|bucket| !bucket.is_empty()) {
            unordered.push(async { (self.get_insertion_candidates(&bucket[0]).await, bucket) });
        }

        let to_removes: Vec<_> = unordered.collect().await;
        {
            let mut write_lock = self.routing_table.write().await;
            for (to_remove, bucket) in to_removes {
                self.remove_and_insert_into_tree(to_remove, bucket, &mut write_lock)
                    .await;
            }
        }
    }

    /// pipelines the three stages for the nodes which allows all to complete
    /// their write, read, write stages in synchronicity when inserting.
    #[instrument(skip_all)]
    pub async fn add_nodes_without_removing(&self, nodes: impl IntoIterator<Item = Node>) {
        let mut write_lock = self.routing_table.write().await;

        let nodes: Vec<_> = nodes
            .into_iter()
            .filter(|v| v.id() != self.local_node.id())
            .filter(|n| {
                let dist_pair: DistancePair<Node, ID_LEN> = (n.clone(), self.local_addr()).into();
                let leaf = write_lock.get_leaf(dist_pair.distance());
                !leaf.contains(&dist_pair)
            })
            .collect();

        // now if there are any leftover, they are all alive nodes that
        // oveflowed the siblings list
        let leftover: Vec<_> = self
            .maybe_add_nodes_to_siblings_list(nodes, &mut write_lock)
            .into_iter()
            .collect();

        for pair in leftover {
            self.insert_into_tree(pair, &mut write_lock);
        }
    }

    /// pipelines the three stages for the nodes which allows all to complete
    /// their write, read, write stages in synchronicity when inserting.
    #[instrument(skip_all)]
    #[cfg(test)]
    pub(crate) async fn load_nodes_in(&self, nodes: impl IntoIterator<Item = Node>) {
        let mut write_lock = self.routing_table.write().await;

        let nodes = nodes
            .into_iter()
            .inspect(|n| {
                write_lock.mark_bucket_as_looked_up(&self.local_addr().xor_distance(n.id()));
            })
            .collect::<Vec<_>>();

        // now if there are any leftover, they are all alive nodes that
        // oveflowed the siblings list
        let leftover: Vec<_> = self
            .maybe_add_nodes_to_siblings_list(nodes, &mut write_lock)
            .into_iter()
            .collect();

        for pair in leftover {
            self.insert_into_tree(pair, &mut write_lock);
        }
    }

    pub async fn find_node(&self, from: Node, id: &Id<ID_LEN>) -> Vec<Node> {
        self.add_node(from.clone()).await;
        let lock = self.routing_table.read().await;
        let mut out: Vec<DistancePair<Node, ID_LEN>> = self
            .find_node_with_lock(id, &lock)
            .map(|pair| pair.node())
            .cloned()
            .map(|node| DistancePair::from((node, id)))
            .collect();

        out.sort();
        out.truncate(BUCKET_SIZE);

        Vec::from_iter(out.into_iter().map(DistancePair::into_node))
    }
    #[instrument(level = "trace", skip_all, fields(%id, dst=%id^self.local_addr()))]
    pub async fn node_lookup(&self, id: &Id<ID_LEN>) -> Vec<Node> {
        trace!("nli started");
        let mut closest_nodes: Vec<DistancePair<Node, ID_LEN>> = {
            let mut lock = self.routing_table.write().await;
            lock.mark_bucket_as_looked_up(&(self.local_node.id() ^ id));
            let lock = lock.downgrade();
            self.find_node_with_lock(id, &lock)
                .map(|pair| DistancePair::from((pair.node().clone(), id)))
                .collect()
        };
        closest_nodes.sort();
        closest_nodes.truncate(BUCKET_SIZE);
        trace!(?closest_nodes);
        let queried_ids = RwLock::new(HashSet::<Id<ID_LEN>>::new());
        let alpha_closest: Vec<Node> = closest_nodes
            .iter()
            .map(|p| p.node())
            .take(ALPHA)
            .cloned()
            .collect();
        let k_closest = RwLock::new(closest_nodes);
        let querying = FuturesUnordered::new();
        let queried: Vec<_> = alpha_closest.iter().map(HasId::id).cloned().collect();
        for node in alpha_closest {
            querying.push(self.node_lookup_inner(node, id, &k_closest, &queried_ids));
        }
        // wait for all to finish, don't care about result
        trace!("awaiting query");
        querying.count().await;
        trace!("query finished");
        let mut write_lock = queried_ids.write().await;
        write_lock.extend(queried);
        // stage 2 where we continously query all k remaining which haven't been
        // queried until all k remaining have been queried
        let mut remaining: Vec<_> = {
            let queried_ids_lock = write_lock.downgrade();

            k_closest
                .read()
                .await
                .iter()
                .filter(|p| !queried_ids_lock.contains(p.node().id()))
                .map(|pair| pair.node())
                .cloned()
                .collect()
        };

        while !remaining.is_empty() {
            trace!("querying remaining {} unqueried nodes", remaining.len());
            let querying = FuturesUnordered::new();
            {
                let mut queried_ids_lock = queried_ids.write().await;
                for node in remaining {
                    let span = trace_span!("querying remaining", ?node);
                    queried_ids_lock.insert(node.id().clone());
                    querying.push(
                        self.update_k_closest_nodes(node, id, &k_closest)
                            .instrument(span),
                    );
                }
            }
            // await all queries, don't care about their results
            querying.count().await;

            remaining = {
                let queried_ids_lock = queried_ids.read().await;
                k_closest
                    .read()
                    .await
                    .iter()
                    .filter(|p| !queried_ids_lock.contains(p.node().id()))
                    .map(|pair| pair.node())
                    .cloned()
                    .collect()
            }
        }

        k_closest
            .into_inner()
            .into_iter()
            .map(|p| p.into_node())
            .collect()
    }
    /// In the recursive step, the initiator resends the FIND_NODE to nodes it
    /// has learned about from previous RPCs. (This recursion can begin
    /// before all alpha of the previous RPCs have returned). Of the k nodes
    /// the initiator has heard of closest to the target, it picks alpha
    /// that it has not yet queried and resends the FIND_NODE RPC to them.
    /// Nodes that fail to respond quickly are removed from consideration
    /// until and unless they do respond. If a round of FIND_NODEs fails to
    /// return a node any closer than the closest already seen, the
    /// initiator resends the FIND_NODE to all of the k closest nodes *it
    /// has not already queried*. The lookup terminates when the
    /// initiator has queried and gotten responses from the k closest nodes it
    /// has seen. When a = 1, the lookup algorithm resembles Chord's in terms of
    /// message cost and the latency of detecting failed nodes. However,
    /// Kademlia can route for lower latency because it has the flexibility
    /// of choosing any one of k nodes to forward a request to.
    ///
    /// k_closest_nodes should be sorted based on the distance to the
    /// target_id.
    ///
    /// returns whether any closer nodes were found in this iteration
    #[instrument(level = "trace", name = "nli", fields(dst=%node.id().xor_distance(target_id)), skip_all)]
    async fn node_lookup_inner(
        &self,
        node: Node,
        target_id: &Id<ID_LEN>,
        k_closest: &RwLock<Vec<DistancePair<Node, ID_LEN>>>,
        queried_node_ids: &RwLock<HashSet<Id<ID_LEN>>>,
    ) -> bool {
        let (k_closest_lock, out) = self
            .update_k_closest_nodes(node, target_id, k_closest)
            .await;

        trace!(?k_closest_lock);

        if !out {
            return false;
        }

        let queried_node_ids_lock = queried_node_ids.read().await;
        let next_to_query = k_closest_lock
            .downgrade()
            .iter()
            .filter(|&pair| !queried_node_ids_lock.contains(pair.node().id()))
            .map(|pair| pair.node().clone())
            .take(ALPHA)
            .collect::<Vec<_>>();
        if next_to_query.is_empty() {
            // round either succeeded and found more or it didn't.
            // Either way, recursive call is now over since there are no more nodes to query
            return out;
        }

        let queried_ids: Vec<_> = next_to_query
            .iter()
            .map(|node| node.id())
            .cloned()
            .collect();
        trace!(querying=?next_to_query.iter().map(|n| DistancePair::from((n.clone(), target_id))).collect::<Vec<_>>());
        drop(queried_node_ids_lock);
        let querying = FuturesUnordered::from_iter(
            next_to_query
                .into_iter()
                .map(|node| self.node_lookup_inner(node, target_id, k_closest, queried_node_ids)),
        );

        trace!("querying");
        let round_succeeded = querying.any(|b| async move { b }).await;
        trace!("finished querying");

        queried_node_ids.write().await.extend(queried_ids);

        if !round_succeeded {
            out // even if the sub-queries didn't, we still might have succeeded
        } else {
            true // round found more
        }
    }

    async fn update_k_closest_nodes<'a>(
        &self,
        node: Node,
        target_id: &Id<ID_LEN>,
        k_closest: &'a RwLock<Vec<DistancePair<Node, ID_LEN>>>,
    ) -> (
        tokio::sync::RwLockWriteGuard<'a, Vec<DistancePair<Node, ID_LEN>>>,
        bool,
    ) {
        trace!("finding nearest nodes");
        let closest_nodes = self
            .handler
            .find_node(&self.local_node, &node, target_id)
            .await;

        trace!(closest_nodes=?closest_nodes.iter().cloned().map(|n| DistancePair::from((n, target_id))).collect::<Vec<_>>());

        // try to add all these nodes
        self.add_nodes(closest_nodes.clone()).await;
        let mut k_closest = k_closest.write().await;

        let init_len = k_closest.len();

        let farthest_k_dist = k_closest[k_closest.len() - 1].distance().clone();

        let closest_nodes = closest_nodes
            .clone()
            .into_iter()
            .map(|node| DistancePair::from((node, target_id)))
            .filter(|pair| pair.distance() < &farthest_k_dist);
        k_closest.extend(closest_nodes);
        k_closest.sort();
        k_closest.dedup();
        let out = k_closest.len() > init_len;
        k_closest.truncate(BUCKET_SIZE);

        (k_closest, out)
    }

    fn find_node_with_lock<'a>(
        &self,
        id: &Id<ID_LEN>,
        lock: &'a impl Deref<Target = RoutingTable<Node, ID_LEN, BUCKET_SIZE>>,
    ) -> impl Iterator<Item = &'a DistancePair<Node, ID_LEN>>
    where
        Node: 'a,
    {
        let dist = self.local_addr().xor_distance(id);
        lock.find_node(dist)
    }

    #[cfg(test)]
    pub(crate) async fn to_parts(&self) -> (Node, Vec<Node>) {
        (
            self.local_node.clone(),
            self.routing_table
                .read()
                .await
                .everything()
                .into_iter()
                .map(|pair| pair.into_node())
                .collect(),
        )
    }

    #[cfg(test)]
    pub(crate) async fn from_parts_unchecked(
        handler: Handler,
        local_node: Node,
        nodes: impl IntoIterator<Item = Node>,
    ) -> Self {
        let manager = Self::new(handler, local_node);
        manager.load_nodes_in(nodes).await;
        manager
    }
}

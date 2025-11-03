use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    ops::Not,
    sync::Arc,
};

use futures::{prelude::*, stream::FuturesUnordered};
use futures_time::{future::FutureExt, time::Duration};
// sync is runtime agnostic;
// see https://docs.rs/tokio/latest/tokio/sync/index.html#runtime-compatibility
use tokio::sync::{RwLock, RwLockWriteGuard};
use tracing::{Instrument, instrument, trace, trace_span};

use crate::{
    HasId, RoutingTable,
    id::{Distance, DistancePair, Id},
    routing_table,
    traits::RequestHandler,
};

const ALPHA: usize = 3;

/// key is assumed to be an Id<ID_LEN>
pub struct RpcManager<
    Node: HasId<ID_LEN>,
    Handler: RequestHandler<Node, ID_LEN>,
    const ID_LEN: usize,
    const BUCKET_SIZE: usize,
> {
    handler: Handler,
    routing_table: Arc<RwLock<RoutingTable<Node, ID_LEN, BUCKET_SIZE>>>,
    local_node: Node,
}

impl<
    Node: HasId<ID_LEN> + Clone,
    Handler: RequestHandler<Node, ID_LEN> + Clone,
    const ID_LEN: usize,
    const BUCKET_SIZE: usize,
> Clone for RpcManager<Node, Handler, ID_LEN, BUCKET_SIZE>
{
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            routing_table: self.routing_table.clone(),
            local_node: self.local_node.clone(),
        }
    }
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
    // 2.3: Refreshing means picking a random ID (just gonna do halfway) in the
    // bucket's range and performing a node search for that ID.
    pub async fn refresh_buckets_after(&self, after: &Distance<ID_LEN>) {
        // perform node_lookup on the bucket directly after the input distance.
        // could also simply do a left shift instead
        let lz_count = after.leading_zeros();
        let shift_by = ID_LEN * 8 - lz_count;
        let lookups = FuturesUnordered::new();
        for shift_by in shift_by..(shift_by + lz_count) {
            let shifted_one: Distance<ID_LEN> = Distance::ONE << shift_by;
            let next_bucket_addr = after + &shifted_one;
            // reverse engineer distance to id
            let target_id = &next_bucket_addr ^ self.local_addr();
            lookups.push(async move { self.node_lookup(&target_id).await });
        }
        // await all refreshes
        lookups.count().await;
    }
    // 2.3: To join the network, a node u must have a contact to an already
    // participating node w. u inserts w into the appropriate k-bucket. u then
    // performs a node lookup for its own node ID. Finally, u refreshes all
    // k-buckets further away than its closest neighbor. During the refreshes, u
    // both populates its own k-buckets and inserts itself into other nodes'
    // k-buckets as necessary.
    #[instrument(skip(self))]
    pub async fn join_network(&self) {
        self.node_lookup(&self.local_node.id()).await;
        // trace!("Post self lookup sib list", &self.local_nod)
        let farthest_dist_seen = {
            let routing_table = self.routing_table.read().await;
            routing_table
                .find_node(&Distance::MAX)
                .max()
                .or_else(|| routing_table.nearest_in_sibling_list(&Distance::MAX).max())
                .map(|v| v.distance())
                .unwrap_or(&Distance::ZERO)
                .clone()
        };

        self.refresh_buckets_after(&farthest_dist_seen).await;
    }

    fn local_addr(&self) -> &Id<ID_LEN> {
        &self.local_node.id()
    }

    pub async fn add_node(&self, node: Node) {
        self.add_nodes([node]).await
    }

    async fn remove_and_insert(
        &self,
        to_remove: Vec<Distance<ID_LEN>>,
        pair: DistancePair<Node, ID_LEN>,
        lock: &mut RwLockWriteGuard<'_, RoutingTable<Node, ID_LEN, BUCKET_SIZE>>,
    ) {
        let leaf = lock.get_leaf_mut(pair.distance());
        leaf.remove_where(|pair| to_remove.contains(pair.distance()));
        // since we either made room or returned early, we can safely
        // assume that the leaf likely has room when doing this insertion.
        let _ = leaf.try_insert(pair);
    }

    async fn get_removal_candidates(
        &self,
        pair: &DistancePair<Node, ID_LEN>,
    ) -> Option<Vec<Distance<ID_LEN>>> {
        let routing_table = self.routing_table.read().await;
        let leaf = routing_table.get_leaf(pair.distance());
        let leaf_len = leaf.len();
        // probably smart to batch this

        if leaf.is_full() {
            let mut failed_to_ping: Vec<Distance<_>> = vec![];
            {
                let unordered_futures = FuturesUnordered::new();
                for pair in leaf.iter() {
                    unordered_futures.push(async move {
                        (
                            self.handler.ping(&self.local_node, pair.node()).await,
                            pair.distance(),
                        )
                    });
                }
                let mut unordered_futures_chunks = unordered_futures.ready_chunks(leaf_len);
                while let Some(finished_pings) = unordered_futures_chunks.next().await {
                    for (ping_succeeded, distance) in finished_pings {
                        if !ping_succeeded {
                            failed_to_ping.push(distance.clone());
                        }
                    }
                    if failed_to_ping.len() != 0 {
                        // we've freed up at least some nodes
                        break;
                    }
                }
            }
            if failed_to_ping.len() == 0 {
                // no room was freed up, all nodes in bucket were online
                return None;
            }
            Some(failed_to_ping)
        } else {
            Some(vec![])
        }
    }

    fn maybe_add_node_to_siblings_list(
        &self,
        node: Node,
        table_lock: &mut RwLockWriteGuard<'_, RoutingTable<Node, ID_LEN, BUCKET_SIZE>>,
    ) -> Result<(), routing_table::Error<Node, ID_LEN>> {
        let pair: DistancePair<Node, ID_LEN> = (node, self.local_addr()).into();
        table_lock.maybe_add_node_to_siblings_list(pair)
    }

    /// pipelines the three stages for the nodes which allows all to complete
    /// their write, read, write stages in synchronicity when inserting.
    #[instrument(skip_all)]
    pub async fn add_nodes(&self, nodes: impl IntoIterator<Item = Node>) {
        let mut write_lock = self.routing_table.write().await;
        let leftover: Vec<_> = nodes
            .into_iter()
            .map(|node| self.maybe_add_node_to_siblings_list(node, &mut write_lock))
            .filter_map(|node| node.err())
            .collect();

        let leftover: Vec<_> = if leftover
            .iter()
            .any(|err| matches!(err, routing_table::Error::Full(_)))
        {
            // try to make room in siblings list. If any room was made,
            // replace leftover with another call. If no room could be made,
            // treat all like they should be inserted into the tree
            let sibling_list_pairs = write_lock.sibling_list_pairs().cloned().collect::<Vec<_>>();
            let pings = FuturesUnordered::new();
            for pair in sibling_list_pairs.iter() {
                pings.push(async {
                    (
                        pair.node().id().clone(),
                        self.handler.ping(&self.local_node, pair.node()).await,
                    )
                });
            }
            let mut chunked_pings = pings.ready_chunks(BUCKET_SIZE);
            let space_made = loop {
                let Some(ping_chunk) = chunked_pings.next().await else {
                    break false;
                };
                let unreachable_nodes: HashSet<_> = ping_chunk
                    .into_iter()
                    .filter_map(|(node_id, is_alive)| (!is_alive).then_some(node_id))
                    .collect();
                let mut removed_any = false;
                write_lock.remove_siblings_list_nodes_where(|pair| {
                    let should_remove = unreachable_nodes.contains(pair.node().id());
                    removed_any = should_remove || removed_any;
                    should_remove
                });
                if removed_any {
                    break true;
                } // otherwise do another loop
            };
            let iter: Box<dyn Iterator<Item = routing_table::Error<Node, ID_LEN>>> = if space_made {
                // try adding nodes one last time to the list
                Box::new(leftover.into_iter().filter_map(|err| {
                    self.maybe_add_node_to_siblings_list(
                        DistancePair::from(err).into_node(),
                        &mut write_lock,
                    )
                    .err()
                }))
            } else {
                Box::new(leftover.into_iter())
            };
            iter.filter_map(|e| match e {
                routing_table::Error::Full(_pair) => None,
                routing_table::Error::OutOfRange(pair) => Some(pair),
            })
            .collect()
        } else {
            leftover.into_iter().map(|e| e.into()).collect()
        };

        drop(write_lock);

        let unordered = FuturesUnordered::new();
        for pair in leftover {
            unordered.push(async { (self.get_removal_candidates(&pair).await, pair) });
        }
        let unordered = unordered.filter_map(async |v| {
            if let Some(to_remove) = v.0 {
                Some((to_remove, v.1))
            } else {
                None
            }
        });

        let to_removes: Vec<_> = unordered.collect().await;
        {
            let mut write_lock = self.routing_table.write().await;
            for (to_remove, pair) in to_removes {
                self.remove_and_insert(to_remove, pair, &mut write_lock)
                    .await;
            }
        }
    }

    pub async fn find_node(&self, from: Node, id: &Id<ID_LEN>) -> Vec<Node> {
        self.add_node(from.clone()).await;
        let lock = self.routing_table.read().await;
        let mut out: Vec<DistancePair<Node, ID_LEN>> = self
            .find_node_with_lock(id, &*lock)
            .map(|pair| pair.node())
            .cloned()
            .map(|node| DistancePair::from((node, id)))
            .collect();

        out.sort();
        out.truncate(BUCKET_SIZE);

        Vec::from_iter(out.into_iter().map(DistancePair::into_node))
    }
    #[instrument(level = "trace", skip_all, fields(%id))]
    pub async fn node_lookup(&self, id: &Id<ID_LEN>) -> Vec<Node> {
        let mut closest_nodes: Vec<DistancePair<Node, ID_LEN>> = {
            let lock = self.routing_table.read().await;
            self.find_node_with_lock(id, &lock)
                .map(|pair| DistancePair::from((pair.node().clone(), id)))
                .collect()
        };
        closest_nodes.sort();
        closest_nodes.truncate(ALPHA);
        let queried_ids = RwLock::new(HashSet::<Id<ID_LEN>>::new());
        let alpha_closest: Vec<Node> = closest_nodes
            .iter()
            .take(ALPHA)
            .map(|p| p.node())
            .cloned()
            .collect();
        let k_closest = RwLock::new(closest_nodes);
        let querying = FuturesUnordered::new();
        for node in alpha_closest {
            queried_ids.write().await.insert(node.id().clone());
            querying.push(self.node_lookup_inner(node, id, &k_closest, &queried_ids));
        }
        // wait for all to finish, don't care about result
        trace!("awaiting query");
        querying.count().await;
        trace!("query finished");
        // stage 2 where we continously query all k remaining which haven't been
        // queried until all k remaining have been queried
        let mut remaining: Vec<_> = {
            let queried_ids_lock = queried_ids.read().await;

            k_closest
                .read()
                .await
                .iter()
                .filter(|p| !queried_ids_lock.contains(p.node().id()))
                .map(|pair| pair.node())
                .cloned()
                .collect()
        };

        while remaining.len() != 0 {
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
        let (k_closest_lock, out) = match self
            .update_k_closest_nodes(node, target_id, k_closest)
            .await
        {
            Some(v) => v,
            None => return false,
        };

        let mut queried_node_ids_lock = queried_node_ids.write().await;
        let next_to_query = k_closest_lock
            .downgrade()
            .iter()
            .filter_map(|pair| {
                (!queried_node_ids_lock.contains(pair.node().id())).then(|| pair.node().clone())
            })
            .take(ALPHA)
            .collect::<Vec<_>>();
        if next_to_query.len() == 0 {
            // round either succeeded and found more or it didn't.
            // Either way, recursive call is now over since there are no more nodes to query
            return out;
        }
        trace!(querying=?next_to_query.iter().map(|n| DistancePair::from((n.clone(), target_id))).collect::<Vec<_>>());
        queried_node_ids_lock.extend(next_to_query.iter().map(|node| node.id()).cloned());
        drop(queried_node_ids_lock);
        let querying = FuturesUnordered::new();
        for node in next_to_query {
            querying.push(self.node_lookup_inner(node, target_id, k_closest, queried_node_ids));
        }
        trace!("querying");
        let round_succeeded = querying.any(|b| async move { b }).await;
        trace!("finished querying");
        if !round_succeeded {
            out // even if the sub-queries didn't, we still succeeded
        } else {
            true // round found more
        }
    }
    #[instrument(skip(self, k_closest))]
    async fn update_k_closest_nodes<'a>(
        &self,
        node: Node,
        target_id: &Id<ID_LEN>,
        k_closest: &'a RwLock<Vec<DistancePair<Node, ID_LEN>>>,
    ) -> Option<(RwLockWriteGuard<'a, Vec<DistancePair<Node, ID_LEN>>>, bool)> {
        let closest_nodes = self
            .handler
            .find_node(&self.local_node, &node, target_id)
            .timeout(Duration::from_secs(5))
            .await
            .ok()?;
        // try to add all these nodes
        self.add_nodes(closest_nodes.iter().cloned().collect::<Vec<_>>())
            .await;

        let mut k_closest = k_closest.write().await;
        let init_len = k_closest.len();

        let closest_nodes = closest_nodes
            .into_iter()
            .map(|node| DistancePair::from((node, target_id)));
        k_closest.extend(closest_nodes);
        k_closest.sort();
        k_closest.dedup();
        k_closest.truncate(BUCKET_SIZE);

        let out = k_closest.len() > init_len;

        Some((k_closest, out))
    }

    fn find_node_with_lock<'a>(
        &self,
        id: &Id<ID_LEN>,
        lock: &'a RoutingTable<Node, ID_LEN, BUCKET_SIZE>,
    ) -> Box<dyn Iterator<Item = &'a DistancePair<Node, ID_LEN>> + 'a> {
        let dist = self.local_addr().xor_distance(id);
        lock.find_node(&dist)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        hash::{DefaultHasher, Hash, Hasher},
        sync::{LazyLock, atomic::AtomicU64},
        time::{Duration, Instant},
    };

    use futures::{StreamExt as _, stream::FuturesUnordered};
    use futures_time::task::sleep;
    use tokio::sync::RwLock;
    use tracing::{instrument, instrument::WithSubscriber as _, subscriber::NoSubscriber, trace};
    use tracing_test::traced_test;

    use crate::{HasId, Id, RpcManager, id::DistancePair, traits::RequestHandler};
    use std::fmt::Debug;
    #[derive(Default)]
    pub struct HandlerInstance {
        nodes: HashSet<Node>,
        managers: HashMap<Id<ID_LEN>, RpcManager<Node, Handler, ID_LEN, BUCKET_SIZE>>,
        ping_cache: RwLock<HashMap<Id<ID_LEN>, (Instant, bool)>>,
    }

    static HANDLER: LazyLock<RwLock<HandlerInstance>> =
        LazyLock::new(|| RwLock::new(HandlerInstance::default()));

    static TOTAL_RPCS: AtomicU64 = AtomicU64::new(0);
    static SHOULD_KEEP_ROTATING: LazyLock<RwLock<bool>> = LazyLock::new(|| RwLock::new(true));

    pub struct Handler;

    impl HandlerInstance {
        pub fn add_node(&mut self, node: Node) -> &RpcManager<Node, Handler, ID_LEN, BUCKET_SIZE> {
            let manager = RpcManager::new(Handler, node.clone());
            self.managers.insert(node.id().clone(), manager);
            let manager_ref = self.managers.get(&node.id()).unwrap();
            self.nodes.insert(node);
            manager_ref
        }

        pub fn manager_of(
            &self,
            id: &Id<ID_LEN>,
        ) -> &RpcManager<Node, Handler, ID_LEN, BUCKET_SIZE> {
            self.managers.get(id).unwrap()
        }
    }

    const BUCKET_SIZE: usize = 20;
    const ID_LEN: usize = 8;

    #[derive(Clone, PartialEq, Eq)]
    pub struct Node {
        name: String,
        id: Id<ID_LEN>,
    }

    impl Hash for Node {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.name.hash(state);
        }
    }

    impl Debug for Node {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Node").field("name", &self.name).finish()
        }
    }

    impl<T: Into<String>> From<T> for Node {
        fn from(value: T) -> Self {
            let name: String = value.into();
            let mut hasher = DefaultHasher::new();
            name.hash(&mut hasher);
            let id = hasher.finish();
            Self {
                name,
                id: id.to_be_bytes().into(),
            }
        }
    }

    impl HasId<ID_LEN> for Node {
        fn id(&self) -> &Id<ID_LEN> {
            &self.id
        }
    }

    impl RequestHandler<Node, ID_LEN> for Handler {
        #[instrument(level = "trace", skip(self))]
        async fn ping(&self, from: &Node, node: &Node) -> bool {
            // add random latency; since all call ping, this adds latency
            // to all calls
            if !*SHOULD_KEEP_ROTATING.read().await {
                trace!("SLEEPING ON PING");
                sleep(Duration::from_millis(rand::random_range(0..1000)).into()).await;
            }
            let reader = HANDLER.read().await;
            let ping_reader = reader.ping_cache.read().await;
            let Some(cache) = ping_reader.get(&node.id().clone()) else {
                return false; // not online yet
            };
            let out = cache.1;
            return out;
        }
        #[instrument(level = "trace", skip(self))]
        async fn find_node(
            &self,
            from: &Node,
            to: &Node,
            address: &crate::id::Id<ID_LEN>,
        ) -> Vec<Node> {
            if self.ping(from, to).await == false {
                return vec![];
            }
            TOTAL_RPCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let read_lock = HANDLER.read().await;
            let Some(manager) = read_lock.managers.get(to.id()) else {
                return vec![];
            };
            manager.find_node(from.clone(), address).await
        }
    }
    #[tokio::test(flavor = "multi_thread")]
    #[traced_test]
    async fn test() {
        trace!("STARTING TEST");
        let mut handler = HANDLER.write().await;
        trace!("GOT HANDLER LOCK");
        // generate 1000 nodes, add them, then have all connect to the manager
        let mut nodes: Vec<Node> = vec![];
        trace!("Made vec");

        for n in 0..100_000 {
            let node = Node::from(n.to_string());
            nodes.push(node.clone());

            handler
                .ping_cache
                .write()
                .await
                .insert(node.id().clone(), (Instant::now(), true));

            let manager = handler.add_node(node.clone());
            // add 1% of the nodes to each node
            manager
                .add_nodes((0..100).map(|_| Node::from(rand::random_range(0..100_000).to_string())))
                .await;

            tokio::spawn(async move {
                let mut node_is_online = true;
                let mut last_changed = Instant::now();
                sleep(Duration::from_millis(rand::random_range(500..100_000)).into()).await;
                loop {
                    sleep(Duration::from_micros(rand::random_range(1..50_000)).into()).await;
                    let dur_since_change = last_changed.duration_since(last_changed).as_micros();
                    // otherwise, update cache

                    let chance_of_changing_online_status =
                        0.5 + 0.5 * (1. - 1. / (0.001 * (dur_since_change as f64) + 1.));
                    let changed_status = rand::random_bool(chance_of_changing_online_status);
                    if changed_status {
                        node_is_online = !node_is_online;
                        last_changed = Instant::now();
                        HANDLER
                            .read()
                            .await
                            .ping_cache
                            .write()
                            .await
                            .entry(node.id().clone())
                            .and_modify(|v| v.1 = node_is_online)
                            .or_insert((last_changed, node_is_online));
                        if node_is_online {
                            // trace!("bringing node {:?} online", node);
                            // just came online, should probably tell
                            // everyone again!
                            let handler = HANDLER.read().await;
                            let my_manager = handler.manager_of(node.id());
                            my_manager
                                .join_network()
                                .with_subscriber(NoSubscriber::new())
                                .await;
                            // trace!("brought node {:?} online", node);
                        }
                    };
                    while !*SHOULD_KEEP_ROTATING.read().await {
                        sleep(Duration::from_millis(rand::random_range(0..3000)).into()).await;
                    }
                }
            });
        }
        // handler.add_node(main_node.clone());
        trace!("made list of nodes");
        let read_handler = handler.downgrade();
        let join_net = FuturesUnordered::new();
        for node in nodes.iter() {
            let manager = read_handler.manager_of(node.id());
            join_net.push(manager.join_network());
        }

        join_net.count().await;

        println!(
            "Total number of rpcs: {}",
            TOTAL_RPCS.load(std::sync::atomic::Ordering::Acquire)
        );
        trace!("finished joining");
        *SHOULD_KEEP_ROTATING.write().await = false;
        sleep(Duration::new(1, 0).into()).await;

        // pings all nodes

        let mut sorted_dist_nodes = nodes
            .iter()
            .cloned()
            .map(|node| DistancePair::from((node, &Id::from([0u8; 8]))))
            .collect::<Vec<_>>();
        let pings = FuturesUnordered::new();
        for pair in sorted_dist_nodes.iter() {
            pings.push(async {
                (
                    Handler.ping(&Node::from("1"), pair.node()).await,
                    pair.node().clone(),
                )
            });
        }
        // remove all that don't fit
        let down_nodes: HashSet<_> = pings
            .filter_map(|v| async move { (!v.0).then_some(v.1) })
            .collect()
            .await;
        sorted_dist_nodes = Vec::from_iter(
            sorted_dist_nodes
                .drain(0..)
                .filter(|node| !down_nodes.contains(node.node())),
        );
        sorted_dist_nodes.sort();
        let sorted_dist_nodes: Vec<_> = sorted_dist_nodes.into_iter().take(BUCKET_SIZE).collect();

        // start of rpc
        let rpc_start = Instant::now();
        trace!("created all handlers");
        let manager = read_handler.manager_of(Node::from("50").id());
        manager.add_nodes(nodes.clone()).await;
        // try to get nodes nearest addr 0
        let nearby = manager.node_lookup(&Id::from([0u8; 8])).await;

        trace!(
            "queried nearby: {:#?}",
            nearby
                .into_iter()
                .map(|node| DistancePair::from((node, &Id::from([0u8; 8]))))
                .collect::<Vec<_>>()
        );
        trace!(?sorted_dist_nodes);

        trace!("Total time to query: {:?}", rpc_start.elapsed());
    }
}

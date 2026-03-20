use std::{
    collections::HashSet,
    fmt::Debug,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use futures::{prelude::*, stream::FuturesUnordered};
// sync is runtime agnostic;
// see https://docs.rs/tokio/latest/tokio/sync/index.html#runtime-compatibility
use tokio::sync::RwLock;
use tracing::{Instrument, debug, instrument, trace, trace_span};

use crate::{
    DistancePair, HasId, Id, RequestHandler,
    node_cache::{MaintnenceLookupAddrs, NodeCache},
    traits::HasServerId,
};

const ALPHA: usize = 3;

/// key is assumed to be an Id<ID_LEN>
///
/// Clone is weak

pub struct RpcManager<
    Client: HasServerId<Node, ID_LEN>,
    Node: HasId<ID_LEN>,
    Handler: RequestHandler<Client, Node, ID_LEN>,
    Cache: NodeCache<Client, Node, ID_LEN>,
    const ID_LEN: usize,
    const BUCKET_SIZE: usize,
> {
    handler: Handler,
    pub(crate) cache: Arc<RwLock<Cache>>,
    local: Client,
    _marker: std::marker::PhantomData<Node>,
}

impl<
    Client: HasServerId<Node, ID_LEN>,
    Node: HasId<ID_LEN>,
    Handler: RequestHandler<Client, Node, ID_LEN>,
    Cache: NodeCache<Client, Node, ID_LEN>,
    const ID_LEN: usize,
    const BUCKET_SIZE: usize,
> Clone for RpcManager<Client, Node, Handler, Cache, ID_LEN, BUCKET_SIZE>
where
    Handler: Clone,
    Node: Clone,
    Client: Clone,
{
    /// weak clone
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            cache: self.cache.clone(),
            local: self.local.clone(),
            _marker: Default::default(),
        }
    }
}

impl<
    Client: HasServerId<Node, ID_LEN> + Clone,
    Node: Eq + HasId<ID_LEN> + Clone + Debug + Eq,
    Handler: RequestHandler<Client, Node, ID_LEN>,
    Cache: NodeCache<Client, Node, ID_LEN>,
    const ID_LEN: usize,
    const BUCKET_SIZE: usize,
> RpcManager<Client, Node, Handler, Cache, ID_LEN, BUCKET_SIZE>
{
    pub fn new(handler: Handler, local_node: Client, cache: Cache) -> Self {
        Self {
            handler,
            cache: Arc::new(cache.into()),
            local: local_node,
            _marker: Default::default(),
        }
    }

    pub async fn cache(&self) -> impl Deref<Target = Cache> {
        self.cache.read().await
    }

    pub async fn cache_mut(&self) -> impl DerefMut<Target = Cache> {
        self.cache.write().await
    }

    // 2.3: To join the network, a node u must have a contact to an already
    // participating node w. u inserts w into the appropriate k-bucket. u then
    // performs a node lookup for its own node ID. Finally, u refreshes all
    // k-buckets further away than its closest neighbor. During the refreshes, u
    // both populates its own k-buckets and inserts itself into other nodes'
    // k-buckets as necessary.
    #[instrument(skip(self))]
    pub async fn join_network(&self)
    where
        Client: MaintnenceLookupAddrs<Client, Node, Cache, ID_LEN>,
    {
        // we always at least start by looking up the node to get a basis of the net around us
        self.node_lookup(&self.local.server_id()).await;
        let bootstrap_addrs = self.local.bootstrap_addrs(self.cache.read().await);
        let lookups = FuturesUnordered::new();
        for addr in bootstrap_addrs {
            lookups.push(async move {
                self.node_lookup(&addr).await;
            });
        }
        lookups.count().await;
    }

    pub async fn add_node(&self, node: Node) {
        self.add_nodes(vec![node]).await
    }

    /// pipelines the three stages for the nodes which allows all to complete
    /// their write, read, write stages in synchronicity when inserting.
    #[instrument(skip_all)]
    pub async fn add_nodes(&self, nodes: Vec<Node>) {
        let removal_candidates = {
            let read_lock = self.cache.read().await;

            read_lock
                .find_removal_candidates(nodes.clone().into_iter(), &self.handler)
                .await
        };

        let mut write_lock = self.cache.write().await;

        write_lock.remove_candidates(removal_candidates);

        write_lock.add_nodes(nodes);
    }

    /// pipelines the two stages for the nodes which allows all to complete
    /// their read, write stages in synchronicity when inserting.
    #[instrument(skip_all)]
    pub async fn add_nodes_without_removing(&self, nodes: impl IntoIterator<Item = Node>) {
        let mut write_lock = self.cache.write().await;
        write_lock.add_nodes(nodes);
    }

    pub async fn find_node(&self, from: Option<Node>, id: &Id<ID_LEN>) -> Vec<Node>
    where
        Node: Send,
    {
        trace!("FINDING NODES");
        let pinned: std::pin::Pin<Box<_>> = Box::pin(async {
            if let Some(from) = from {
                self.add_node(from.clone()).await;
            }
            let lock = self.cache.read().await;
            let mut out: Vec<DistancePair<Node, ID_LEN>> = lock
                .nearby_nodes(id)
                .cloned()
                .map(|node| DistancePair::from((node, id)))
                .collect();

            out.sort();
            out.truncate(BUCKET_SIZE);

            Vec::from_iter(out.into_iter().map(DistancePair::into_node))
        });

        let out = pinned.await;
        trace!(?out);

        return out;
    }
    #[instrument(level = "debug", skip_all, fields(%id, dst=%id^&self.local.server_id()))]
    pub async fn node_lookup(&self, id: &Id<ID_LEN>) -> Vec<Node> {
        let mut closest_nodes: Vec<DistancePair<Node, ID_LEN>> = {
            let mut lock = self.cache.write().await;
            lock.on_node_lookup(id);
            let lock = lock.downgrade();
            lock.nearby_nodes(id)
                .map(|node| DistancePair::from((node.clone(), id)))
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
        // stage 2 where we continuously query all k remaining which haven't been
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
        k_closest.write().await.truncate(BUCKET_SIZE);
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
        let closest_nodes = self.handler.find_node(&self.local, &node, target_id).await;

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
}

mod bucket_list;
mod leaf;

use std::cmp::min;
use std::collections::hash_map;
use std::sync::LazyLock;
use std::time::Instant;

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Debug;
use thiserror::Error;

use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use tracing::instrument;

use crate::routing_table::leaf::Leaf;
use crate::{
    HasId, RequestHandler,
    id::{self, Distance, DistancePair},
};

use bucket_list::{BucketList, LeafMut};

const NEARBY_NODES_MULTIP: usize = 5;

static STARTUP_INSTANT: LazyLock<Instant> = LazyLock::new(Instant::now);

pub struct RoutingTable<Node: HasId<ID_LEN>, const ID_LEN: usize, const BUCKET_SIZE: usize = 20> {
    tree: BucketList<Node, ID_LEN, BUCKET_SIZE>,
    // buckets: buckets::Buckets<Node, ID_LEN, BUCKET_SIZE>,
    // the nearest 5*k nodes to me, binary searched & inserted
    // because of the likelihood of having poor rotating codes
    nearest_siblings_list: Vec<id::DistancePair<Node, ID_LEN>>,
    // when each bucket has last been looked_up
    bucket_updated_at: HashMap<usize, Instant>,
}
#[derive(Debug, Error)]
pub enum Error<Node, const ID_LEN: usize> {
    OutOfRange(DistancePair<Node, ID_LEN>),
}

impl<Node: Debug + HasId<ID_LEN> + Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Debug
    for RoutingTable<Node, ID_LEN, BUCKET_SIZE>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoutingTable")
            .field("nearest_siblings_list", &self.nearest_siblings_list)
            .field("tree", &self.tree)
            .field("bucket_updated_at", &self.bucket_updated_at)
            .finish()
    }
}

impl<Node, const ID_LEN: usize> From<Error<Node, ID_LEN>> for DistancePair<Node, ID_LEN> {
    fn from(value: Error<Node, ID_LEN>) -> Self {
        match value {
            Error::OutOfRange(pair) => pair,
        }
    }
}

impl<Node: Eq + Debug + HasId<ID_LEN>, const ID_LEN: usize, const BUCKET_SIZE: usize> Default
    for RoutingTable<Node, ID_LEN, BUCKET_SIZE>
{
    fn default() -> Self {
        Self {
            tree: BucketList::new(),
            nearest_siblings_list: Vec::with_capacity(NEARBY_NODES_MULTIP * BUCKET_SIZE + 1),
            bucket_updated_at: Default::default(),
        }
    }
}

/// to only take the min ownership necessary over bucket_updated_at
/// rather than taking ownership over all of `self`
macro_rules! bucket_updated_at_entry {
    ($owner:expr, $distance:expr) => {
        Self::bucket_entry(&mut $owner.bucket_updated_at, $distance)
    };
}

impl<Node: Eq + Debug + HasId<ID_LEN>, const ID_LEN: usize, const BUCKET_SIZE: usize>
    RoutingTable<Node, ID_LEN, BUCKET_SIZE>
{
    pub fn new() -> Self {
        Self {
            tree: BucketList::new(),
            nearest_siblings_list: Vec::with_capacity(NEARBY_NODES_MULTIP * BUCKET_SIZE + 1),
            bucket_updated_at: Default::default(),
        }
    }
    /// Gets a [leaf](Leaf) based on a provided [distance](Distance).
    /// Useful to possibly add a node to a bucket.
    /// Since kademlia recommends potentially pinging each node before inserting
    /// into a specific leaf (when the leaf is full), insertions should be done
    /// directly on the returned leaf.
    pub fn get_leaf_mut<'a>(
        &'a mut self,
        distance: &Distance<ID_LEN>,
    ) -> LeafMut<'a, Node, ID_LEN, BUCKET_SIZE>
    where
        Node: HasId<ID_LEN>,
    {
        bucket_updated_at_entry!(self, distance).or_insert(*STARTUP_INSTANT);
        self.tree.get_leaf_mut(distance)
    }

    /// Gets a [leaf](Leaf) based on a provided [distance](Distance).
    /// Useful to get a list of references to a given leaf.
    pub fn get_leaf(&self, distance: &Distance<ID_LEN>) -> &Leaf<Node, ID_LEN, BUCKET_SIZE>
    where
        Node: HasId<ID_LEN>,
    {
        self.tree.get_leaf(distance)
    }

    #[cfg(test)]
    #[allow(unused)]
    pub(crate) fn sibling_list(&self) -> &Vec<DistancePair<Node, ID_LEN>> {
        &self.nearest_siblings_list
    }

    #[cfg(test)]
    #[allow(unused)]
    pub(crate) fn everything(&self) -> Vec<DistancePair<Node, ID_LEN>>
    where
        Node: Clone,
    {
        let mut out: Vec<DistancePair<Node, ID_LEN>> = Vec::new();
        out.extend(self.sibling_list().iter().cloned());
        out.extend(self.tree.nodes_near(Distance::ZERO, usize::MAX).cloned());
        out.sort();
        out
    }

    pub fn get_stale_buckets(&self, duration: &std::time::Duration) -> impl Iterator<Item = usize> {
        self.bucket_updated_at
            .iter()
            .filter(|(_, at)| {
                std::time::Instant::now().duration_since(**at) < *duration
                    || **at == *STARTUP_INSTANT
            })
            .map(|(idx, _)| *idx)
    }
    /// tries to add nodes to the siblings list.
    /// Returns any nodes which got evicted from the list.
    pub fn maybe_add_nodes_to_siblings_list<DP, Iter: IntoIterator<Item = DP>>(
        &mut self,
        pairs: Iter,
    ) -> impl IntoIterator<Item = DistancePair<Node, ID_LEN>>
    where
        DistancePair<Node, ID_LEN>: From<DP>,
    {
        let last_in_list: &Distance<_> = &self
            .nearest_siblings_list
            .get(NEARBY_NODES_MULTIP * BUCKET_SIZE)
            .map(|p| p.distance().clone())
            .unwrap_or(Distance::MAX);
        let iter = pairs
            .into_iter()
            .map(DistancePair::from)
            .filter(|v| v.distance() < last_in_list);
        // insert a new instant in the distant past to any newly seen bucket
        let iter = iter.inspect(|pair| {
            bucket_updated_at_entry!(self, pair.distance()).or_insert(*STARTUP_INSTANT);
        });
        self.nearest_siblings_list.extend(iter);
        self.nearest_siblings_list.sort();
        self.nearest_siblings_list.dedup();
        self.nearest_siblings_list.drain(
            min(
                NEARBY_NODES_MULTIP * BUCKET_SIZE,
                self.nearest_siblings_list.len(),
            )..self.nearest_siblings_list.len(),
        )
    }

    pub fn sibling_list_pairs(&self) -> impl Iterator<Item = &DistancePair<Node, ID_LEN>> {
        self.nearest_siblings_list.iter()
    }

    fn bucket_id(distance: &Distance<ID_LEN>) -> usize {
        distance.leading_zeros()
    }

    fn bucket_entry<'a>(
        bucket_updated_at: &'a mut HashMap<usize, Instant>,
        distance: &Distance<ID_LEN>,
    ) -> hash_map::Entry<'a, usize, std::time::Instant> {
        bucket_updated_at.entry(Self::bucket_id(distance))
    }

    pub(crate) fn mark_bucket_as_looked_up(&mut self, distance: &Distance<ID_LEN>) {
        bucket_updated_at_entry!(self, distance).insert_entry(Instant::now());
    }

    pub async fn remove_unreachable_siblings_list_nodes(
        &mut self,
        local_node: &Node,
        handler: &impl RequestHandler<Node, ID_LEN>,
    ) {
        // Ping all nodes concurrently and collect the ones that respond.
        let mut to_remove_set = HashSet::new();
        {
            let to_remove = FuturesUnordered::from_iter(self.nearest_siblings_list.iter().map(
                |pair| async move {
                    (!handler.ping(local_node, pair.node()).await)
                        .then_some(pair.node().id().clone())
                },
            ));

            // once at least half have responded, continue.
            let mut chunks = to_remove.ready_chunks(BUCKET_SIZE);
            let mut total_pinged = 0;
            while let Some(chunk) = chunks.next().await {
                total_pinged += chunk.len();
                to_remove_set.extend(chunk.into_iter().flatten());
                if total_pinged >= BUCKET_SIZE {
                    break;
                }
            }
        }

        self.nearest_siblings_list = self
            .nearest_siblings_list
            .drain(..)
            .filter(|pair| !to_remove_set.contains(pair.node().id()))
            .collect();
    }
    /// dist should be distance from the target to the local_node
    #[instrument(skip_all)]
    pub fn nearest_in_sibling_list(
        &self,
        dist: &Distance<ID_LEN>,
        count: usize,
    ) -> std::slice::Iter<'_, DistancePair<Node, ID_LEN>> {
        let nearest_index = self
            .nearest_siblings_list
            .binary_search_by_key(&dist, |pair| pair.distance())
            .unwrap_or_else(|err| err);

        let start_of_range = nearest_index.saturating_sub(count / 2);
        let end_of_range = min(nearest_index + count / 2, self.nearest_siblings_list.len()); // returns all nearest_siblings
        self.nearest_siblings_list[start_of_range..end_of_range].iter()
    }

    pub fn find_node<'a, 'b>(
        &'a self,
        dist: Distance<ID_LEN>,
    ) -> impl Iterator<Item = &'a DistancePair<Node, ID_LEN>> + 'a
    where
        'a: 'b,
    {
        self.nearest_in_sibling_list(&dist, BUCKET_SIZE)
            .chain(self.tree.nodes_near(dist, BUCKET_SIZE))
    }

    pub fn find_node_mut(
        &mut self,
        pair: &DistancePair<Node, ID_LEN>,
    ) -> Option<&mut DistancePair<Node, ID_LEN>> {
        self.nearest_siblings_list
            .binary_search(pair)
            .ok()
            .map(|ind| &mut self.nearest_siblings_list[ind])
            .or_else(
                || match self.tree.nodes_near_mut(pair.distance(), 1).next() {
                    Some(out) if out == pair => Some(out),
                    _ => None,
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use expect_test::expect;
    use tracing::trace;
    use tracing_test::traced_test;

    use crate::{HasId, RoutingTable, node::Node};
    #[test]
    #[traced_test]
    pub fn siblings() {
        let mut table: RoutingTable<_, _, 1> = RoutingTable::new();
        // generate 1k nodes in a loop
        let self_node = Node::new("0.0.0.0:0".parse().unwrap());
        for _ in 0..10 {
            let nodes = (1..1001).map(|port| {
                (
                    Node::new(format!("127.0.0.1:{port}").parse().unwrap()),
                    self_node.id(),
                )
            });
            table.maybe_add_nodes_to_siblings_list(nodes);
        }
        expect![[r#"
            [
                DistancePair(
                    0035...8F8D,
                    Node {
                        addr: 127.0.0.1:833,
                        id: "Id(90C2...7074)",
                    },
                ),
                DistancePair(
                    0040...AA40,
                    Node {
                        addr: 127.0.0.1:351,
                        id: "Id(90B7...55B9)",
                    },
                ),
                DistancePair(
                    005B...50E1,
                    Node {
                        addr: 127.0.0.1:221,
                        id: "Id(90AC...AF18)",
                    },
                ),
                DistancePair(
                    00BA...83A4,
                    Node {
                        addr: 127.0.0.1:345,
                        id: "Id(904D...7C5D)",
                    },
                ),
                DistancePair(
                    00D1...156A,
                    Node {
                        addr: 127.0.0.1:789,
                        id: "Id(9026...EA93)",
                    },
                ),
            ]
        "#]]
        .assert_debug_eq(table.sibling_list());
        trace!("{:#?}", table.sibling_list());
    }
}

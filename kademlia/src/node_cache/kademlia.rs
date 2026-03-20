use futures::StreamExt;
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
};
use tracing::{instrument, trace};

use crate::{DistancePair, HasId, Id, RequestHandler, RoutingTable};

pub struct NodeCache<Node: HasId<ID_LEN>, const ID_LEN: usize, const BUCKET_SIZE: usize = 20> {
    table: RoutingTable<Node, ID_LEN, BUCKET_SIZE>,
    local: Node,
}

impl<Node: Debug + Eq + HasId<ID_LEN>, const ID_LEN: usize, const BUCKET_SIZE: usize>
    super::Cullable<Node, Node, ID_LEN> for NodeCache<Node, ID_LEN, BUCKET_SIZE>
{
    type CullSet = (HashSet<Id<ID_LEN>>, HashMap<usize, HashSet<Id<ID_LEN>>>);

    fn remove_candidates(&mut self, candidates: Self::CullSet) {
        self.table.remove_siblings_list_nodes(&candidates.0);
        for (_, node_ids) in candidates.1.iter() {
            let Some(first) = node_ids.iter().next() else {
                continue;
            };
            let dist = first.id().xor_distance(self.local.id());
            let mut leaf = self.table.get_leaf_mut(&dist);
            leaf.remove_where(|n| node_ids.contains(n.node().id()));
        }
    }

    async fn find_removal_candidates(
        &self,
        nodes: impl IntoIterator<Item = Node>,
        handler: &impl RequestHandler<Node, Node, ID_LEN>,
    ) -> Self::CullSet {
        let offline_siblings = self
            .table
            .find_dead_siblings_list_nodes(&self.local, handler)
            .await;
        // leading zeroes to distance pair
        let mut buckets_to_remove_from: HashMap<usize, Vec<Node>> = HashMap::new();
        for node in nodes.into_iter() {
            let leading_zeros = node.id().xor_distance(self.local.id()).leading_zeros();
            buckets_to_remove_from
                .entry(leading_zeros)
                .or_default()
                .push(node);
        }

        let mut leaf_removals: HashMap<usize, HashSet<Id<ID_LEN>>> = Default::default();

        for (bucket_id, nodes) in buckets_to_remove_from.iter() {
            let Some(first) = nodes.iter().next() else {
                continue;
            };
            let leaf = self
                .table
                .get_leaf(&first.id().xor_distance(self.local.id()));
            let futures = leaf
                .iter()
                .map(|pair| async {
                    handler
                        .ping(&self.local, pair.node())
                        .await
                        .then(|| pair.node().id().clone())
                })
                .collect::<futures::stream::FuturesUnordered<_>>();
            let removal_addrs: HashSet<Id<ID_LEN>> =
                futures.filter_map(|f| async { f }).collect().await;
            leaf_removals.insert(*bucket_id, removal_addrs);
        }

        (offline_siblings, leaf_removals)
    }
}

impl<Node, const ID_LEN: usize, const BUCKET_SIZE: usize> super::NodeCache<Node, Node, ID_LEN>
    for NodeCache<Node, ID_LEN, BUCKET_SIZE>
where
    Node: HasId<ID_LEN> + Debug + Eq + Clone,
{
    #[instrument(skip(self, nodes))]
    fn add_nodes(&mut self, nodes: impl IntoIterator<Item = Node>) {
        trace!("before {:?}", self.table);

        let nodes: Vec<_> = nodes
            .into_iter()
            .filter(|v| v.id() != self.local.id())
            .filter(|n| {
                let dist_pair: DistancePair<Node, ID_LEN> = (n.clone(), self.local.id()).into();
                let leaf = self.table.get_leaf(dist_pair.distance());
                !leaf.contains(&dist_pair)
            })
            .collect();

        trace!(?nodes);

        // now if there are any leftover, they are all alive nodes that
        // overflowed the siblings list

        let leftover: Vec<_> = {
            let local_addr = self.local.id();
            let pairs = nodes.into_iter().map(move |node| (node, local_addr));
            self.table
                .maybe_add_nodes_to_siblings_list(pairs)
                .into_iter()
                .collect()
        };

        // now insert the leftover into the specific leaf node
        // leading zeroes to distance pair
        let mut buckets: HashMap<usize, Vec<_>> = HashMap::new();
        for pair in leftover.into_iter() {
            let leading_zeros = pair.distance().leading_zeros();
            buckets.entry(leading_zeros).or_default().push(pair);
        }
        for pairs in buckets.into_values() {
            let mut leaf = self.table.get_leaf_mut(&pairs[0].distance());
            // safe to silently ignore because RPC always tries to remove inactive nodes
            // before calling add_nodes
            for pair in pairs {
                let _res = leaf.try_insert(pair);
            }
        }
        trace!("after {:?}", self.table);
    }

    fn on_node_lookup(&mut self, id: &Id<ID_LEN>) {
        let dist = self.local.id().xor_distance(id);
        self.table.mark_bucket_as_looked_up(&dist);
    }

    fn nearby_nodes<'a>(&'a self, address: &Id<ID_LEN>) -> impl Iterator<Item = &'a Node>
    where
        Node: 'a,
    {
        self.table
            .find_node(address.xor_distance(self.local.id()))
            .map(|v| v.node())
    }
}

impl<Node: HasId<ID_LEN>, const ID_LEN: usize, const BUCKET_SIZE: usize>
    NodeCache<Node, ID_LEN, BUCKET_SIZE>
{
    pub fn new(local: Node) -> NodeCache<Node, ID_LEN, BUCKET_SIZE> {
        Self {
            table: Default::default(),
            local,
        }
    }
}

mod tree;
use std::cmp::min;

use std::fmt::Debug;
use thiserror::Error;

use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;

use crate::{
    HasId, RequestHandler,
    id::{self, Distance, DistancePair},
};

use tree::{Leaf, Tree};

pub use tree::Bucket;

pub struct RoutingTable<Node, const ID_LEN: usize, const BUCKET_SIZE: usize = 20>
where
    Node: HasId<ID_LEN>,
{
    tree: tree::Tree<Node, ID_LEN, BUCKET_SIZE>,
    // the nearest 5*k nodes to me, binary searched & inserted
    // because of the likelihood of having poor rotating codes
    nearest_siblings_list: Vec<id::DistancePair<Node, ID_LEN>>,
}
#[derive(Debug, Error)]
pub enum Error<Node, const ID_LEN: usize> {
    OutOfRange(DistancePair<Node, ID_LEN>),
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
            tree: Tree::new(0),
            nearest_siblings_list: Vec::with_capacity(5 * BUCKET_SIZE + 1),
        }
    }
}

impl<Node: Eq + Debug + HasId<ID_LEN>, const ID_LEN: usize, const BUCKET_SIZE: usize>
    RoutingTable<Node, ID_LEN, BUCKET_SIZE>
{
    pub fn new() -> Self {
        Self {
            tree: Tree::new(0),
            nearest_siblings_list: Vec::with_capacity(5 * BUCKET_SIZE + 1),
        }
    }
    /// Gets a [leaf](Leaf) based on a provided [distance](Distance).
    /// Useful to possibly add a node to a bucket.
    /// Since kademlia recommends potentially pinging each node before inserting
    /// into a specific leaf (when the leaf is full), insertions should be done
    /// directly on the returned leaf.
    pub fn get_leaf_mut(
        &mut self,
        distance: &Distance<ID_LEN>,
    ) -> &mut Leaf<Node, ID_LEN, BUCKET_SIZE>
    where
        Node: HasId<ID_LEN>,
    {
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
        out.extend(self.tree.nodes_near(&Distance::ZERO, usize::MAX).cloned());
        out.sort();
        out
    }
    /// tries to add nodes to the siblings list.
    /// Returns any nodes which got evicted from the list.
    pub fn maybe_add_nodes_to_siblings_list<
        DP: Into<DistancePair<Node, ID_LEN>>,
        Iter: IntoIterator<Item = DP>,
    >(
        &mut self,
        pair: Iter,
    ) -> impl IntoIterator<Item = DistancePair<Node, ID_LEN>> {
        let iter = pair.into_iter();
        self.nearest_siblings_list.extend(iter.map(Into::into));
        self.nearest_siblings_list.sort();
        self.nearest_siblings_list.drain(
            min(5 * BUCKET_SIZE, self.nearest_siblings_list.len())
                ..self.nearest_siblings_list.len(),
        )
    }

    pub fn sibling_list_pairs(&self) -> impl Iterator<Item = &DistancePair<Node, ID_LEN>> {
        self.nearest_siblings_list.iter()
    }

    pub async fn remove_unreachable_siblings_list_nodes(
        &mut self,
        local_node: &Node,
        handler: &impl RequestHandler<Node, ID_LEN>,
    ) {
        // Ping all nodes concurrently and collect the ones that respond.
        let mut futures = FuturesUnordered::new();

        for pair in self.nearest_siblings_list.drain(0..) {
            // Capture references to `handler` and `local_node` by reference.
            // `pair` is moved into the async block so we can return it if ping succeeds.
            let fut = async move {
                if handler.ping(local_node, pair.node()).await {
                    Some(pair)
                } else {
                    None
                }
            };
            futures.push(fut);
        }

        let mut new_list = Vec::with_capacity(self.nearest_siblings_list.capacity());
        while let Some(maybe_pair) = futures.next().await {
            if let Some(pair) = maybe_pair {
                new_list.push(pair);
            }
        }

        self.nearest_siblings_list = new_list;
    }

    pub fn nearest_in_sibling_list(
        &self,
        dist: &Distance<ID_LEN>,
    ) -> Box<dyn Iterator<Item = &DistancePair<Node, ID_LEN>> + '_> {
        // maybe include some from sib list
        let nearest = match self
            .nearest_siblings_list
            .binary_search_by(|p| p.distance().cmp(dist))
        {
            Ok(v) => v,
            Err(v) => v,
        };
        // get at least BUCKET_SIZE from nearest_in_sib_list
        Box::new(
            self.nearest_siblings_list
                .iter()
                .skip(min(
                    nearest.saturating_sub(BUCKET_SIZE / 2),
                    self.nearest_siblings_list.len().saturating_sub(BUCKET_SIZE),
                ))
                .take(BUCKET_SIZE),
        )
    }

    pub fn find_node(
        &self,
        dist: &Distance<ID_LEN>,
    ) -> Box<dyn Iterator<Item = &DistancePair<Node, ID_LEN>> + '_> {
        Box::new(
            self.nearest_in_sibling_list(dist)
                .chain(self.tree.nodes_near(dist, BUCKET_SIZE)),
        )
    }

    pub fn find_node_mut(
        &mut self,
        pair: &DistancePair<Node, ID_LEN>,
    ) -> Option<&mut DistancePair<Node, ID_LEN>> {
        match self.tree.nodes_near_mut(pair.distance(), 1).next() {
            Some(out) if out == pair => Some(out),
            _ => None,
        }
    }
}

mod tree;
use std::cmp::min;

use std::fmt::Debug;
use thiserror::Error;
use tracing::warn;

use crate::{
    HasId,
    id::{self, Distance, DistancePair},
    routing_table::tree::{Leaf, Tree},
};

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
    Full(DistancePair<Node, ID_LEN>),
    OutOfRange(DistancePair<Node, ID_LEN>),
}

impl<Node, const ID_LEN: usize> From<Error<Node, ID_LEN>> for DistancePair<Node, ID_LEN> {
    fn from(value: Error<Node, ID_LEN>) -> Self {
        match value {
            Error::Full(pair) => pair,
            Error::OutOfRange(pair) => pair,
        }
    }
}

impl<Node: Eq + Debug, const ID_LEN: usize, const BUCKET_SIZE: usize>
    RoutingTable<Node, ID_LEN, BUCKET_SIZE>
where
    Node: HasId<ID_LEN>,
{
    pub fn new() -> Self {
        Self {
            tree: Tree::new(0),
            nearest_siblings_list: Vec::with_capacity(5 * BUCKET_SIZE),
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

    fn nearest_siblings_insert(
        &mut self,
        pair: DistancePair<Node, ID_LEN>,
    ) -> Result<(), Error<Node, ID_LEN>> {
        // if in list already, no-op
        if let Ok(_) = self.nearest_siblings_list.binary_search(&pair) {
            return Ok(());
        }
        if self.nearest_siblings_list.len() + 1 > 5 * BUCKET_SIZE {
            return Err(Error::Full(pair));
        }
        self.nearest_siblings_list.push(pair);
        self.nearest_siblings_list.sort();
        for pair in self.nearest_siblings_list.drain(
            min(5 * BUCKET_SIZE, self.nearest_siblings_list.len())
                ..self.nearest_siblings_list.len(),
        ) {
            // likely evicting into closest bucket anyways, and if not then we
            // can still afford potentially losing the occasional close but not
            // close enough pair.
            let res = self.tree.get_leaf_mut(pair.distance()).try_insert(pair);
            if let Err(_e) = res {
                warn!("improperly evicted a node without pinging other nodes in bucket first")
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn sibling_list(&self) -> &Vec<DistancePair<Node, ID_LEN>> {
        &self.nearest_siblings_list
    }

    #[cfg(test)]
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

    /// either adds to the sibling list or returns Some(pair), where pair is the
    /// inputted pair.
    /// If this returns none then it is safe to add the node to the leaf
    /// node in the tree.
    /// ``
    /// table.maybe_add_to_siblings_list(node);
    pub fn maybe_add_node_to_siblings_list(
        &mut self,
        pair: impl Into<DistancePair<Node, ID_LEN>>,
    ) -> Result<(), Error<Node, ID_LEN>> {
        let pair = pair.into();

        if self.nearest_siblings_list.len() == 0 {
            // first item to be added into the list
            // expects that the returned iterator is empty
            // since we just added it in
            self.nearest_siblings_insert(pair)
                .expect("insert to succeed since the list is empty");
            return Ok(());
        };
        let last_in_siblings_list = self
            .nearest_siblings_list
            .get(self.nearest_siblings_list.len() - 1)
            .expect("at least one sibling in the list");
        if let Ok(_) = self.nearest_siblings_list.binary_search(&pair) {
            return Ok(()); // already added
        }
        if last_in_siblings_list.distance() > pair.distance() {
            // we can insert into siblings since we're closer than the farthest.
            // might wanna try inserting the displaced node
            self.nearest_siblings_insert(pair);
            return Ok(());
        }
        Err(Error::OutOfRange(pair))
    }

    pub fn sibling_list_pairs(&self) -> impl Iterator<Item = &DistancePair<Node, ID_LEN>> {
        self.nearest_siblings_list.iter()
    }

    pub fn remove_siblings_list_nodes_where<F: FnMut(&DistancePair<Node, ID_LEN>) -> bool>(
        &mut self,
        pred: F,
    ) {
        let filtered: Vec<_> = self.nearest_siblings_list.drain(0..).filter(pred).collect();
        self.nearest_siblings_list.extend(filtered);
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
                    nearest.checked_sub(BUCKET_SIZE / 2).unwrap_or(0),
                    self.nearest_siblings_list
                        .len()
                        .checked_sub(BUCKET_SIZE)
                        .unwrap_or(0),
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
                .chain(self.tree.nodes_near(dist, 1)),
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

#[cfg(test)]
mod tests {

    use crate::{
        HasId,
        id::{Distance, DistancePair},
        node::Node,
        routing_table::RoutingTable,
    };

    #[test]
    pub fn insert_example() {
        let local_node = Node::new("127.0.0.1:2000".parse().unwrap());
        let local_id = local_node.id();

        let mut table = RoutingTable::<_, _, 20>::new();
        let n1 = Node::new("0.0.0.0:1".parse().unwrap());
        let n1_pair: DistancePair<Node, _> = (n1, local_id).into();
        let n1_dist = n1_pair.distance().clone();
        if let Err(n1_pair) = table.maybe_add_node_to_siblings_list(n1_pair) {
            // didn't add to siblings list
            let leaf = table.get_leaf_mut(&n1_dist);
            if leaf.is_full() {
                let mut failed_to_ping: Option<Distance<_>> = None;
                // try to make room if it's full, maybe by pinging in async code
                for pair in leaf.iter() {
                    // ping node, add it to dists_to_remove if the ping fails
                    let ping_succeeded = true;
                    if !ping_succeeded {
                        failed_to_ping = Some(pair.distance().clone());
                        break;
                    }
                }
                if let Some(failed_to_ping) = failed_to_ping {
                    leaf.remove_where(|pair| failed_to_ping == *pair.distance());
                } else {
                    // no node got cleared, no need to re-attempt an insertion.
                }
            }
            // since we tried our best to make space if the leaf is full, now we can insert
            // and safely ignore the error if the leaf is still full as kademlia says to
            // drop new nodes heard about if none of the existing nodes are
            let _ = leaf.try_insert(n1_pair);
        }
    }
}

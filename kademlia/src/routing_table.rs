mod tree;
use crate::{
    HasId,
    id::{self, Distance, DistancePair},
    routing_table::tree::{Leaf, Tree},
};

pub struct RoutingTable<Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize = 20>
where
    Node: HasId<ID_LEN>,
{
    tree: tree::Tree<Node, ID_LEN, BUCKET_SIZE>,
    // the nearest 5*k nodes to me, binary searched & inserted
    // because of the likelihood of having poor rotating codes
    nearest_siblings_list: Vec<id::DistancePair<Node, ID_LEN>>,
}

impl<Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize>
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

    fn nearest_siblings_insert(&mut self, pair: DistancePair<Node, ID_LEN>) {
        self.nearest_siblings_list.push(pair);
        self.nearest_siblings_list.sort();
        self.nearest_siblings_list.pop();
        // TODO: might be worth replacing the list with a binary tree
        // might also not be worth it since we don't have that many elements
    }

    /// either adds to the sibling list or returns None.
    /// If this returns none then it is safe to add the node to the leaf
    /// node in the tree.
    /// ``
    /// table.maybe_add_to_siblings_list(node);
    pub fn maybe_add_node_to_siblings_list(
        &mut self,
        pair: impl Into<DistancePair<Node, ID_LEN>>,
    ) -> Option<DistancePair<Node, ID_LEN>> {
        let pair = pair.into();

        if self.nearest_siblings_list.len() == 0 {
            // first item to be added into the list
            self.nearest_siblings_insert(pair);
            return None;
        };
        let last_in_siblings_list = self
            .nearest_siblings_list
            .get(self.nearest_siblings_list.len() - 1)
            .expect("at least one sibling in the list");
        if last_in_siblings_list.distance() > pair.distance() {
            // we can insert into siblings since we're closer than most.
            self.nearest_siblings_insert(pair);
            return None;
        }
        Some(pair)
    }

    pub fn find_nodes_near(&mut self, distance: &Distance<ID_LEN>, count: usize) {}

    pub fn find_node(
        &self,
        pair: &DistancePair<Node, ID_LEN>,
    ) -> Option<&DistancePair<Node, ID_LEN>> {
        match self.tree.nodes_near(pair.distance(), 1).next() {
            Some(out) if out == pair => Some(out),
            _ => None,
        }
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
        if let Some(n1_pair) = table.maybe_add_node_to_siblings_list(n1_pair) {
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

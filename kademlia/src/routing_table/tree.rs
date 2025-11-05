use std::ops::Index;
use std::sync::atomic::AtomicUsize;

use crate::HasId;
use crate::id::{Distance, DistancePair};

pub(crate) struct Tree<Node, const ID_LEN: usize, const BUCKET_SIZE: usize> {
    branch_type: BranchType<Node, ID_LEN, BUCKET_SIZE>,
    // actual cached len might be zero, but recreating a zero length value
    // is a) rare and b) cheap.
    cached_len: AtomicUsize,
    depth: usize,
}

pub(crate) enum BranchType<Node, const ID_LEN: usize, const BUCKET_SIZE: usize> {
    Split {
        left: Box<Tree<Node, ID_LEN, BUCKET_SIZE>>,
        right: Box<Tree<Node, ID_LEN, BUCKET_SIZE>>,
    },
    Leaf(leaf::Leaf<Node, ID_LEN, BUCKET_SIZE>),
}

mod bucket;
mod leaf;

pub(crate) use leaf::Leaf;

pub use leaf::Bucket;

impl<Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Tree<Node, ID_LEN, BUCKET_SIZE> {
    pub fn new(depth: usize) -> Self {
        Self {
            branch_type: BranchType::Leaf(leaf::Leaf::new(false)),
            cached_len: AtomicUsize::new(0),
            depth,
        }
    }

    pub(crate) fn from_leaf(leaf: Leaf<Node, ID_LEN, BUCKET_SIZE>, depth: usize) -> Self {
        Tree {
            branch_type: BranchType::Leaf(leaf),
            cached_len: AtomicUsize::new(0),
            depth,
        }
    }

    fn from_split(left: Box<Self>, right: Box<Self>, depth: usize) -> Self {
        Self {
            branch_type: BranchType::Split { left, right },
            cached_len: AtomicUsize::new(0),
            depth,
        }
    }

    /// checks if the bucket should split and splits if it should.
    fn maybe_split(&mut self) {
        let maybe_new_self = match &mut self.branch_type {
            BranchType::Leaf(leaf) => leaf.maybe_split(self.depth),
            _ => None,
        };
        if let Some(new_self) = maybe_new_self {
            *self = new_self;
        }
    }

    fn maybe_merge(&mut self) {
        let self_len = self.len();
        let new_leaf_node = match &self.branch_type {
            BranchType::Split { .. } if self_len < BUCKET_SIZE / 2 => self.merge_into(),
            // can't merge/isn't worth splitting yet
            _ => return,
        };
        // intrinsically invalidates self len cache since a new node is created
        // in the tree.
        *self = Self::from_leaf(new_leaf_node, self.depth);
    }

    fn merge_into(&mut self) -> Leaf<Node, ID_LEN, BUCKET_SIZE> {
        // perform merge
        // replacement: we know that this is not a left node since left
        // nodes are always leaf nodes.
        let mut leaf = Leaf::new(false);
        for value in self.drain() {
            if leaf.try_insert(value).is_err() {
                unreachable!()
            }
        }
        leaf
    }

    pub fn get_leaf(&self, distance: &Distance<ID_LEN>) -> &Leaf<Node, ID_LEN, BUCKET_SIZE> {
        self.cached_len
            .store(0, std::sync::atomic::Ordering::Release);
        match &self.branch_type {
            BranchType::Split { left, right } => {
                let is_zero = bit_of_array::<ID_LEN>(distance, self.depth);
                if is_zero {
                    right.get_leaf(distance)
                } else {
                    left.get_leaf(distance)
                }
            }
            BranchType::Leaf(leaf) => leaf,
        }
    }

    pub fn get_leaf_mut(
        &mut self,
        distance: &Distance<ID_LEN>,
    ) -> &mut Leaf<Node, ID_LEN, BUCKET_SIZE> {
        self.maybe_split();
        self.maybe_merge();
        self.cached_len
            .store(0, std::sync::atomic::Ordering::Release);
        match &mut self.branch_type {
            BranchType::Split { left, right } => {
                let is_zero = bit_of_array::<ID_LEN>(distance, self.depth);
                if is_zero {
                    right.get_leaf_mut(distance)
                } else {
                    left.get_leaf_mut(distance)
                }
            }
            BranchType::Leaf(leaf) => leaf,
        }
    }

    pub fn nodes_near<const N: usize>(
        &self,
        dist: &Distance<N>,
        length: usize,
    ) -> Box<dyn Iterator<Item = &DistancePair<Node, ID_LEN>> + '_>
    where
        Node: HasId<N>,
    {
        match &self.branch_type {
            BranchType::Split { left, right } => {
                let is_zero = bit_of_array::<ID_LEN>(dist, self.depth + 1);
                // figure out which branch is taken next and check if taking the
                // next branch would be less than the splitting factor.
                let (next_branch, other_branch) = if is_zero {
                    // go to the right
                    (right, left)
                } else {
                    (left, right)
                };
                if next_branch.len() < length {
                    // should stop here since if we recursed farther we'd end up with an iterator
                    // less than that of length.

                    // should return iter for the one nearer to the target id, then the iter of the
                    // one farther from the target id
                    Box::new(next_branch.iter().chain(other_branch.iter()).take(length))
                } else {
                    next_branch.nodes_near(dist, length)
                }
            }
            // base case; either this node has enough to satisfy length or the entire tree does not
            // have enough to satisfy length and the first node in the tree is a leaf node.
            BranchType::Leaf(leaf) => Box::new(leaf.iter()),
        }
    }

    pub fn nodes_near_mut<const N: usize>(
        &mut self,
        dist: &Distance<N>,
        length: usize,
    ) -> Box<dyn Iterator<Item = &mut DistancePair<Node, ID_LEN>> + '_> {
        match &mut self.branch_type {
            BranchType::Split { left, right } => {
                let is_zero = bit_of_array::<ID_LEN>(dist, self.depth + 1);
                // figure out which branch is taken next and check if taking the
                // next branch would be less than the splitting factor.
                let (next_branch, other_branch) = if is_zero {
                    // go to the right
                    (right, left)
                } else {
                    (left, right)
                };
                if next_branch.len() < length {
                    // should stop recursion here since if we recursed farther we'd end up with an
                    // iterator less than that of length.

                    // should return iter for the one nearer to the id, then the iter of the one
                    // farther from the id
                    Box::new(next_branch.iter_mut().chain(other_branch.iter_mut()))
                } else {
                    next_branch.nodes_near_mut(dist, length)
                }
            }
            // base case; either this node has enough to satisfy length or the entire tree does not
            // have enough to satisfy length and the first node in the tree is a leaf node.
            BranchType::Leaf(leaf) => Box::new(leaf.iter_mut()),
        }
    }
    /// Iterates by joining recursively, iterating roughly from right (closest)
    /// to left (furthest)
    pub fn iter(&self) -> Box<dyn Iterator<Item = &DistancePair<Node, ID_LEN>> + '_> {
        match &self.branch_type {
            BranchType::Split { left, right } => Box::new(right.iter().chain(left.iter())),
            BranchType::Leaf(leaf) => Box::new(leaf.iter()),
        }
    }

    /// Iterates by joining recursively, iterating roughly from right (closest)
    /// to left (furthest)
    pub fn iter_mut(&mut self) -> Box<dyn Iterator<Item = &mut DistancePair<Node, ID_LEN>> + '_> {
        match &mut self.branch_type {
            BranchType::Split { left, right } => Box::new(right.iter_mut().chain(left.iter_mut())),
            BranchType::Leaf(leaf) => Box::new(leaf.iter_mut()),
        }
    }

    pub fn drain(&mut self) -> Box<dyn Iterator<Item = DistancePair<Node, ID_LEN>> + '_> {
        match &mut self.branch_type {
            BranchType::Split { left, right } => Box::new(right.drain().chain(left.drain())),
            BranchType::Leaf(leaf) => Box::new(leaf.drain()),
        }
    }

    /// Gets the total number of elements in this subtree recursively.
    /// Recursing the tree for the length can be expensive to do regularly,
    /// hence caching which is invalidated whenever a leaf is received from this
    /// tree.
    /// This means that repeat requests for the length in one search down the
    /// tree is cached so long as no mutable leaf is held as a reference.
    //
    // Rust's safety guarantees this fact since calling the len() function
    // guarantees that there are currently no references to any child objects,
    // such as leaves, held by the caller.
    //
    // Since the caller also cannot get any reference to a tree node, thanks to
    // them being restricted to this crate, we can safely assume that consumers
    // of the crate have no way but through the root node to get any given leaf.
    // Furthermore, since cached lengths are only invalidated when a leaf below
    // them is gotten, the cache will remain for all branches not explored since
    // the last cache time. This significantly reduces recalculations of the
    // length calculation since at worst, only log(n) of the nodes' length caches
    // will be invalidated on any given leaf node fetch.
    pub fn len(&self) -> usize {
        let cached = self.cached_len.load(std::sync::atomic::Ordering::Acquire);
        if cached != 0 {
            return cached;
        }

        let len = match &self.branch_type {
            BranchType::Split { left, right } => left.len() + right.len(),
            BranchType::Leaf(leaf) => leaf.len(),
        };
        self.cached_len
            .store(len, std::sync::atomic::Ordering::Release);
        len
    }
}

pub(crate) fn bit_of_array<const N: usize>(
    id: &impl Index<usize, Output = u8>,
    index: usize,
) -> bool {
    // shift and keep the lowest (shifted) bit
    if index > N * 8 {
        panic!("indexing too far into an id");
    }
    (id[index / 8] >> (7 - (index % 8)) & 0b1) != 0
}

#[cfg(test)]
pub(crate) mod tests {

    use super::bit_of_array;

    #[test]
    pub fn test_bit_from_id() {
        assert_eq!(false, bit_of_array::<1>(&[0], 0));
        assert_eq!(true, bit_of_array::<1>(&[0b10000000], 0));
        assert_eq!(false, bit_of_array::<1>(&[0b01000000], 0));
        assert_eq!(true, bit_of_array::<1>(&[0b01000000], 1));
        assert_eq!(true, bit_of_array::<1>(&[0b00000001], 7));
        assert_eq!(false, bit_of_array::<1>(&[0b00000001], 6));
    }
}

use crate::id::DistancePair;

use std::{
    hint::cold_path,
    time::{Duration, Instant},
};

use super::Tree;

pub use super::bucket::Bucket;

pub struct Leaf<Node, const ID_LEN: usize, const BUCKET_SIZE: usize> {
    bucket: Bucket<Node, ID_LEN, BUCKET_SIZE>,
    on_left: bool,
    last_looked_up: Instant,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("tried to insert into a full leaf node")]
    FullLeaf,
}

impl<Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Leaf<Node, ID_LEN, BUCKET_SIZE> {
    pub fn new(on_left: bool) -> Self {
        Self {
            on_left,
            bucket: Bucket::new(),
            last_looked_up: Instant::now(),
        }
    }

    pub fn looked_up_within(&self, duration: &Duration) -> bool {
        Instant::now().duration_since(self.last_looked_up) < *duration
    }

    pub(crate) fn mark_as_looked_up(&mut self) {
        self.last_looked_up = Instant::now()
    }
    pub(crate) fn split(&mut self, depth: usize) -> Tree<Node, ID_LEN, BUCKET_SIZE> {
        // split bucket based on ids into a new branch
        let left = Box::new(Tree::new(depth + 1));
        let right = Box::new(Tree::new(depth + 1));
        let mut split = Tree::from_split(left, right, depth);
        // recursively re-insert all values formerly in this
        // leaf node below this leaf node
        for value in self.bucket.drain() {
            // note that the get_leaf call below is a possible recursive call
            // into this split function through the Tree's maybe_split function.
            // Means that if another split is needed on a right node created
            // below the newly created split node then another split will
            // occur.
            let leaf = split.get_leaf_mut(value.distance());
            if let Err(Error::FullLeaf) = leaf.try_insert(value) {
                cold_path();
                unreachable!();
            }
        }
        split
    }
    /// Either splits self into a new subtree if a split is due and returns a
    /// new tree to replace this leaf with or returns [None] if this
    /// node either is not empty or is a left node (which never get split).
    pub(crate) fn maybe_split(&mut self, depth: usize) -> Option<Tree<Node, ID_LEN, BUCKET_SIZE>> {
        if self.bucket.is_full() && !self.on_left {
            cold_path();
            Some(self.split(depth))
        } else {
            None
        }
    }
    /// Whether this leaf node is full (either will split on the next get_leaf
    /// or will throw an error on the next attempted insert).
    pub fn is_full(&self) -> bool {
        self.bucket.is_full()
    }

    fn contains(&self, pair: &DistancePair<Node, ID_LEN>) -> bool {
        self.iter().any(|other| other.node() == pair.node())
    }

    /// Errors if this leaf is full. Remove any unresponsive nodes in this
    /// bucket if this bucket is full before trying to insert.
    /// Can safely ignore the returned result if you are aware of the risks of
    /// not cleaning up dead nodes through functions like [remove_nodes_where].
    pub fn try_insert(
        &mut self,
        distance_pair: impl Into<DistancePair<Node, ID_LEN>>,
    ) -> Result<(), Error> {
        let pair = distance_pair.into();
        if self.contains(&pair) {
            return Ok(());
        }
        if !self.is_full() {
            self.bucket.add(pair);
            Ok(())
        } else {
            Err(Error::FullLeaf)
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &DistancePair<Node, ID_LEN>> {
        self.bucket.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut DistancePair<Node, ID_LEN>> {
        self.bucket.iter_mut()
    }

    pub(super) fn drain(&mut self) -> impl Iterator<Item = DistancePair<Node, ID_LEN>> {
        self.bucket.drain()
    }

    pub fn remove_where<F: FnMut(&DistancePair<Node, ID_LEN>) -> bool>(
        &mut self,
        predicate: F,
    ) -> Vec<DistancePair<Node, ID_LEN>> {
        self.bucket.remove_nodes_where(predicate)
    }

    pub fn len(&self) -> usize {
        self.bucket.len()
    }
}

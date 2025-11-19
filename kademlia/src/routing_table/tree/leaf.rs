use crate::id::DistancePair;

pub use super::bucket::Bucket;

pub struct Leaf<Node, const ID_LEN: usize, const BUCKET_SIZE: usize> {
    bucket: Bucket<Node, ID_LEN, BUCKET_SIZE>,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("tried to insert into a full leaf node")]
    FullLeaf,
}

impl<Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Leaf<Node, ID_LEN, BUCKET_SIZE> {
    pub(crate) fn new() -> Self {
        Self {
            bucket: Bucket::new(),
        }
    }

    /// Whether this leaf node is full (either will split on the next get_leaf
    /// or will throw an error on the next attempted insert).
    pub fn is_full(&self) -> bool {
        self.bucket.is_full()
    }

    pub fn contains(&self, pair: &DistancePair<Node, ID_LEN>) -> bool {
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

    pub fn remove_where<F: FnMut(&DistancePair<Node, ID_LEN>) -> bool>(&mut self, predicate: F) {
        self.bucket.remove_nodes_where(predicate)
    }

    pub fn len(&self) -> usize {
        self.bucket.len()
    }
}

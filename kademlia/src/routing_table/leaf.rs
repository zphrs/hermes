use crate::id::DistancePair;

pub struct Leaf<Node, const ID_LEN: usize, const BUCKET_SIZE: usize> {
    bucket: arrayvec::ArrayVec<DistancePair<Node, ID_LEN>, BUCKET_SIZE>,
}

impl<Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Leaf<Node, ID_LEN, BUCKET_SIZE> {
    /// Errors if this leaf is full. Remove any unresponsive nodes in this
    /// bucket if this bucket is full before trying to insert.
    /// Can safely ignore the returned result if you are aware of the risks of
    /// not cleaning up dead nodes through functions like [remove_nodes_where].
    #[inline]
    pub fn try_insert(
        &mut self,
        distance_pair: impl Into<DistancePair<Node, ID_LEN>>,
    ) -> Result<(), Error> {
        let pair = distance_pair.into();
        if self.contains(&pair) {
            return Ok(());
        }

        if !self.is_full() {
            let _ = self.bucket.try_push(pair);
            Ok(())
        } else {
            Err(Error::FullLeaf)
        }
    }

    pub fn contains(&self, pair: &DistancePair<Node, ID_LEN>) -> bool {
        self.bucket.contains(pair)
    }
}

impl<Node, const ID_LEN: usize, const BUCKET_SIZE: usize> Default
    for Leaf<Node, ID_LEN, BUCKET_SIZE>
{
    fn default() -> Self {
        Self {
            bucket: Default::default(),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("tried to insert into a full leaf node")]
    FullLeaf,
}

impl<Node, const ID_LEN: usize, const BUCKET_SIZE: usize> Leaf<Node, ID_LEN, BUCKET_SIZE> {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    /// Whether this leaf node is full (either will split on the next get_leaf
    /// or will throw an error on the next attempted insert).
    pub fn is_full(&self) -> bool {
        self.bucket.is_full()
    }

    pub fn iter(&self) -> impl Iterator<Item = &DistancePair<Node, ID_LEN>> {
        self.bucket.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut DistancePair<Node, ID_LEN>> {
        self.bucket.iter_mut()
    }

    pub(super) fn drain(&mut self) -> impl Iterator<Item = DistancePair<Node, ID_LEN>> {
        self.bucket.drain(..)
    }

    pub fn remove_where<F: FnMut(&mut DistancePair<Node, ID_LEN>) -> bool>(
        &mut self,
        mut predicate: F,
    ) {
        self.bucket.retain(|n| !predicate(n));
    }

    pub fn len(&self) -> usize {
        self.bucket.len()
    }

    /// It is up to the caller to ensure the capacity of the vector is
    /// sufficiently large.
    ///
    /// This method uses debug assertions to check that the arrayvec is not
    /// full.
    pub(super) unsafe fn insert_unchecked(&mut self, element: DistancePair<Node, ID_LEN>) {
        let _ = unsafe { self.bucket.push_unchecked(element) };
    }
}
pub(super) mod private {
    use super::{DistancePair, Leaf};

    impl<Node, const ID_LEN: usize, const BUCKET_SIZE: usize> Extend<DistancePair<Node, ID_LEN>>
        for Leaf<Node, ID_LEN, BUCKET_SIZE>
    {
        fn extend<T: IntoIterator<Item = DistancePair<Node, ID_LEN>>>(&mut self, iter: T) {
            self.bucket.extend(iter);
        }
    }
}

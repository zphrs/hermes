use std::ops::{Deref, DerefMut, Index};
use std::sync::atomic::AtomicUsize;

use crate::HasId;
use crate::id::{Distance, DistancePair};

// pub(crate) struct Tree<Node, const ID_LEN: usize, const BUCKET_SIZE: usize> {
//     branch_type: BranchType<Node, ID_LEN, BUCKET_SIZE>,
//     // actual cached len might be zero, but recreating a zero length value
//     // is a) rare and b) cheap.
//     cached_len: AtomicUsize,
//     depth: usize,
// }

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
use tracing::{instrument, trace};

impl<Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Tree<Node, ID_LEN, BUCKET_SIZE> {
    pub fn new_right(depth: usize) -> Self {
        Self {
            branch_type: BranchType::Leaf(leaf::Leaf::new(false)),
            cached_len: AtomicUsize::new(0),
            depth,
        }
    }

    pub fn new_left(depth: usize) -> Self {
        Self {
            branch_type: BranchType::Leaf(leaf::Leaf::new(true)),
            // makes sure it doesn't get merged instantly
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

    pub fn maybe_merge_recursively(&mut self) {
        if let BranchType::Split { right, .. } = &mut self.branch_type {
            right.maybe_merge_recursively();
        }
        self.maybe_merge();
    }

    fn maybe_merge(&mut self) {
        let self_len = self.len();
        let new_leaf_node = match &self.branch_type {
            BranchType::Split { .. } if self_len < 1 => self.merge_into(),
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
        trace!("merging");
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

    pub fn get_leaf_mut<'a>(
        &'a mut self,
        distance: &Distance<ID_LEN>,
    ) -> LeafMut<'a, Node, ID_LEN, BUCKET_SIZE> {
        LeafMut(self, distance.clone())
    }

    fn get_leaf_raw_mut(
        &mut self,
        distance: &Distance<ID_LEN>,
    ) -> &mut Leaf<Node, ID_LEN, BUCKET_SIZE> {
        self.maybe_split();

        self.cached_len
            .store(0, std::sync::atomic::Ordering::Release);
        match &mut self.branch_type {
            BranchType::Split { left, right } => {
                let is_zero = bit_of_array::<ID_LEN>(distance, self.depth);
                if is_zero {
                    right.get_leaf_raw_mut(distance)
                } else {
                    left.get_leaf_raw_mut(distance)
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
    #[instrument(skip_all)]
    pub fn leaves_iter(&self) -> Box<dyn Iterator<Item = &Leaf<Node, ID_LEN, BUCKET_SIZE>> + '_> {
        match &self.branch_type {
            BranchType::Split { left, right } => {
                trace!("split");
                Box::new(right.leaves_iter().chain(left.leaves_iter()))
            }
            BranchType::Leaf(leaf) => Box::new(LeafIterator::new(leaf)),
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

    fn drain(&mut self) -> Box<dyn Iterator<Item = DistancePair<Node, ID_LEN>> + '_> {
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

struct LeafIterator<'a, Node, const ID_LEN: usize, const BUCKET_SIZE: usize> {
    leaf: &'a Leaf<Node, ID_LEN, BUCKET_SIZE>,
    used: bool,
}

impl<'a, Node, const ID_LEN: usize, const BUCKET_SIZE: usize>
    LeafIterator<'a, Node, ID_LEN, BUCKET_SIZE>
{
    fn new(leaf: &'a Leaf<Node, ID_LEN, BUCKET_SIZE>) -> Self {
        Self { leaf, used: false }
    }
}

impl<'a, Node, const ID_LEN: usize, const BUCKET_SIZE: usize> Iterator
    for LeafIterator<'a, Node, ID_LEN, BUCKET_SIZE>
{
    type Item = &'a Leaf<Node, ID_LEN, BUCKET_SIZE>;

    fn next(&mut self) -> Option<Self::Item> {
        let out = (!self.used).then_some(self.leaf);
        self.used = true;
        out
    }
}

pub struct LeafMut<'a, Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize>(
    &'a mut Tree<Node, ID_LEN, BUCKET_SIZE>,
    Distance<ID_LEN>,
);

impl<'a, Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Drop
    for LeafMut<'a, Node, ID_LEN, BUCKET_SIZE>
{
    fn drop(&mut self) {
        self.0.maybe_merge_recursively();
    }
}

impl<'a, Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> DerefMut
    for LeafMut<'a, Node, ID_LEN, BUCKET_SIZE>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.get_leaf_raw_mut(&self.1)
    }
}
impl<'a, Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Deref
    for LeafMut<'a, Node, ID_LEN, BUCKET_SIZE>
{
    type Target = Leaf<Node, ID_LEN, BUCKET_SIZE>;

    fn deref(&self) -> &Self::Target {
        self.0.get_leaf(&self.1)
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

    use expect_test::expect;
    use tracing_test::traced_test;

    use crate::{DistancePair, HasId, node::Node, routing_table::tree::Tree};

    use super::bit_of_array;

    #[test]
    pub fn test_bit_from_id() {
        assert!(!bit_of_array::<1>(&[0], 0));
        assert!(bit_of_array::<1>(&[0b10000000], 0));
        assert!(!bit_of_array::<1>(&[0b01000000], 0));
        assert!(bit_of_array::<1>(&[0b01000000], 1));
        assert!(bit_of_array::<1>(&[0b00000001], 7));
        assert!(!bit_of_array::<1>(&[0b00000001], 6));
    }

    #[test]
    #[traced_test]
    pub fn test_tree() {
        let mut tree: Tree<Node, 32, 20> = Tree::new_right(0);
        let local_node = Node::new("0.0.0.0:8000".parse().unwrap());
        let nodes = (0..100).map(|port| Node::new(format!("127.0.0.1:{port}").parse().unwrap()));
        for node in nodes {
            let pair: DistancePair<Node, 32> = (node, local_node.id()).into();
            let mut leaf = tree.get_leaf_mut(pair.distance());
            let _ = leaf.try_insert(pair);
        }
        let leaves: Vec<_> = tree
            .leaves_iter()
            .map(|leaf| leaf.iter().collect::<Vec<_>>())
            .collect();
        expect!["4"].assert_eq(&leaves.len().to_string());
        expect![[r#"
            [
                [
                    DistancePair(
                        E809...1DA3,
                        Node {
                            addr: 127.0.0.1:12,
                            id: Id(835E...E52A),
                        },
                    ),
                    DistancePair(
                        E153...BDBE,
                        Node {
                            addr: 127.0.0.1:33,
                            id: Id(8A04...4537),
                        },
                    ),
                    DistancePair(
                        F522...6BB7,
                        Node {
                            addr: 127.0.0.1:36,
                            id: Id(9E75...933E),
                        },
                    ),
                    DistancePair(
                        EA93...D091,
                        Node {
                            addr: 127.0.0.1:50,
                            id: Id(81C4...2818),
                        },
                    ),
                    DistancePair(
                        E4D1...08AB,
                        Node {
                            addr: 127.0.0.1:56,
                            id: Id(8F86...F022),
                        },
                    ),
                    DistancePair(
                        FCD4...FE39,
                        Node {
                            addr: 127.0.0.1:63,
                            id: Id(9783...06B0),
                        },
                    ),
                    DistancePair(
                        FB15...2668,
                        Node {
                            addr: 127.0.0.1:72,
                            id: Id(9042...DEE1),
                        },
                    ),
                    DistancePair(
                        E29B...B623,
                        Node {
                            addr: 127.0.0.1:74,
                            id: Id(89CC...4EAA),
                        },
                    ),
                    DistancePair(
                        E211...F898,
                        Node {
                            addr: 127.0.0.1:76,
                            id: Id(8946...0011),
                        },
                    ),
                    DistancePair(
                        F8C3...BDB6,
                        Node {
                            addr: 127.0.0.1:91,
                            id: Id(9394...453F),
                        },
                    ),
                    DistancePair(
                        E1CB...1CE7,
                        Node {
                            addr: 127.0.0.1:92,
                            id: Id(8A9C...E46E),
                        },
                    ),
                    DistancePair(
                        E964...7D4C,
                        Node {
                            addr: 127.0.0.1:99,
                            id: Id(8233...85C5),
                        },
                    ),
                ],
                [
                    DistancePair(
                        D9F7...4EFA,
                        Node {
                            addr: 127.0.0.1:3,
                            id: Id(B2A0...B673),
                        },
                    ),
                    DistancePair(
                        C7FA...B808,
                        Node {
                            addr: 127.0.0.1:9,
                            id: Id(ACAD...4081),
                        },
                    ),
                    DistancePair(
                        D339...3CEB,
                        Node {
                            addr: 127.0.0.1:15,
                            id: Id(B86E...C462),
                        },
                    ),
                    DistancePair(
                        C93F...E878,
                        Node {
                            addr: 127.0.0.1:16,
                            id: Id(A268...10F1),
                        },
                    ),
                    DistancePair(
                        DACC...A5CE,
                        Node {
                            addr: 127.0.0.1:18,
                            id: Id(B19B...5D47),
                        },
                    ),
                    DistancePair(
                        DF6A...35E4,
                        Node {
                            addr: 127.0.0.1:19,
                            id: Id(B43D...CD6D),
                        },
                    ),
                    DistancePair(
                        CEB7...1BDE,
                        Node {
                            addr: 127.0.0.1:22,
                            id: Id(A5E0...E357),
                        },
                    ),
                    DistancePair(
                        C078...CED7,
                        Node {
                            addr: 127.0.0.1:26,
                            id: Id(AB2F...365E),
                        },
                    ),
                    DistancePair(
                        C485...E6A7,
                        Node {
                            addr: 127.0.0.1:37,
                            id: Id(AFD2...1E2E),
                        },
                    ),
                    DistancePair(
                        C9E2...698F,
                        Node {
                            addr: 127.0.0.1:41,
                            id: Id(A2B5...9106),
                        },
                    ),
                    DistancePair(
                        D883...3DF2,
                        Node {
                            addr: 127.0.0.1:46,
                            id: Id(B3D4...C57B),
                        },
                    ),
                    DistancePair(
                        CFF5...9A14,
                        Node {
                            addr: 127.0.0.1:78,
                            id: Id(A4A2...629D),
                        },
                    ),
                    DistancePair(
                        C55B...4A76,
                        Node {
                            addr: 127.0.0.1:86,
                            id: Id(AE0C...B2FF),
                        },
                    ),
                    DistancePair(
                        C443...7C71,
                        Node {
                            addr: 127.0.0.1:90,
                            id: Id(AF14...84F8),
                        },
                    ),
                    DistancePair(
                        C70D...3E37,
                        Node {
                            addr: 127.0.0.1:95,
                            id: Id(AC5A...C6BE),
                        },
                    ),
                    DistancePair(
                        D23B...0FDD,
                        Node {
                            addr: 127.0.0.1:96,
                            id: Id(B96C...F754),
                        },
                    ),
                    DistancePair(
                        C96E...FFB4,
                        Node {
                            addr: 127.0.0.1:98,
                            id: Id(A239...073D),
                        },
                    ),
                ],
                [
                    DistancePair(
                        B9EC...9C99,
                        Node {
                            addr: 127.0.0.1:4,
                            id: Id(D2BB...6410),
                        },
                    ),
                    DistancePair(
                        9F76...9696,
                        Node {
                            addr: 127.0.0.1:7,
                            id: Id(F421...6E1F),
                        },
                    ),
                    DistancePair(
                        8904...8027,
                        Node {
                            addr: 127.0.0.1:21,
                            id: Id(E253...78AE),
                        },
                    ),
                    DistancePair(
                        AA0E...1127,
                        Node {
                            addr: 127.0.0.1:24,
                            id: Id(C159...E9AE),
                        },
                    ),
                    DistancePair(
                        ABEE...427F,
                        Node {
                            addr: 127.0.0.1:27,
                            id: Id(C0B9...BAF6),
                        },
                    ),
                    DistancePair(
                        AFB5...95EE,
                        Node {
                            addr: 127.0.0.1:30,
                            id: Id(C4E2...6D67),
                        },
                    ),
                    DistancePair(
                        90BB...F817,
                        Node {
                            addr: 127.0.0.1:31,
                            id: Id(FBEC...009E),
                        },
                    ),
                    DistancePair(
                        8DE1...0793,
                        Node {
                            addr: 127.0.0.1:32,
                            id: Id(E6B6...FF1A),
                        },
                    ),
                    DistancePair(
                        A425...B8B1,
                        Node {
                            addr: 127.0.0.1:40,
                            id: Id(CF72...4038),
                        },
                    ),
                    DistancePair(
                        A7DA...2873,
                        Node {
                            addr: 127.0.0.1:43,
                            id: Id(CC8D...D0FA),
                        },
                    ),
                    DistancePair(
                        B920...D30F,
                        Node {
                            addr: 127.0.0.1:44,
                            id: Id(D277...2B86),
                        },
                    ),
                    DistancePair(
                        AADF...BC12,
                        Node {
                            addr: 127.0.0.1:51,
                            id: Id(C188...449B),
                        },
                    ),
                    DistancePair(
                        AB38...D6C0,
                        Node {
                            addr: 127.0.0.1:52,
                            id: Id(C06F...2E49),
                        },
                    ),
                    DistancePair(
                        AE5C...D738,
                        Node {
                            addr: 127.0.0.1:54,
                            id: Id(C50B...2FB1),
                        },
                    ),
                    DistancePair(
                        97FC...7D4C,
                        Node {
                            addr: 127.0.0.1:55,
                            id: Id(FCAB...85C5),
                        },
                    ),
                    DistancePair(
                        9D53...A357,
                        Node {
                            addr: 127.0.0.1:64,
                            id: Id(F604...5BDE),
                        },
                    ),
                    DistancePair(
                        9ED7...58B4,
                        Node {
                            addr: 127.0.0.1:75,
                            id: Id(F580...A03D),
                        },
                    ),
                    DistancePair(
                        992E...E1B7,
                        Node {
                            addr: 127.0.0.1:77,
                            id: Id(F279...193E),
                        },
                    ),
                    DistancePair(
                        8F25...B3B7,
                        Node {
                            addr: 127.0.0.1:80,
                            id: Id(E472...4B3E),
                        },
                    ),
                    DistancePair(
                        A37B...6962,
                        Node {
                            addr: 127.0.0.1:89,
                            id: Id(C82C...91EB),
                        },
                    ),
                ],
                [
                    DistancePair(
                        7B9A...1F70,
                        Node {
                            addr: 127.0.0.1:0,
                            id: Id(10CD...E7F9),
                        },
                    ),
                    DistancePair(
                        1886...AC3D,
                        Node {
                            addr: 127.0.0.1:1,
                            id: Id(73D1...54B4),
                        },
                    ),
                    DistancePair(
                        0F9E...34DE,
                        Node {
                            addr: 127.0.0.1:2,
                            id: Id(64C9...CC57),
                        },
                    ),
                    DistancePair(
                        0E7B...145A,
                        Node {
                            addr: 127.0.0.1:5,
                            id: Id(652C...ECD3),
                        },
                    ),
                    DistancePair(
                        35BF...F877,
                        Node {
                            addr: 127.0.0.1:6,
                            id: Id(5EE8...00FE),
                        },
                    ),
                    DistancePair(
                        6D55...496E,
                        Node {
                            addr: 127.0.0.1:8,
                            id: Id(0602...B1E7),
                        },
                    ),
                    DistancePair(
                        0483...E94F,
                        Node {
                            addr: 127.0.0.1:10,
                            id: Id(6FD4...11C6),
                        },
                    ),
                    DistancePair(
                        718F...B419,
                        Node {
                            addr: 127.0.0.1:11,
                            id: Id(1AD8...4C90),
                        },
                    ),
                    DistancePair(
                        77A8...3DFB,
                        Node {
                            addr: 127.0.0.1:13,
                            id: Id(1CFF...C572),
                        },
                    ),
                    DistancePair(
                        11EB...A7D2,
                        Node {
                            addr: 127.0.0.1:14,
                            id: Id(7ABC...5F5B),
                        },
                    ),
                    DistancePair(
                        4398...D9DB,
                        Node {
                            addr: 127.0.0.1:17,
                            id: Id(28CF...2152),
                        },
                    ),
                    DistancePair(
                        55FC...D9D1,
                        Node {
                            addr: 127.0.0.1:20,
                            id: Id(3EAB...2158),
                        },
                    ),
                    DistancePair(
                        0B8F...67C5,
                        Node {
                            addr: 127.0.0.1:23,
                            id: Id(60D8...9F4C),
                        },
                    ),
                    DistancePair(
                        24BD...97D9,
                        Node {
                            addr: 127.0.0.1:25,
                            id: Id(4FEA...6F50),
                        },
                    ),
                    DistancePair(
                        7EE3...7489,
                        Node {
                            addr: 127.0.0.1:28,
                            id: Id(15B4...9A20),
                        },
                    ),
                    DistancePair(
                        5577...2F96,
                        Node {
                            addr: 127.0.0.1:29,
                            id: Id(3E20...D71F),
                        },
                    ),
                    DistancePair(
                        1B65...F6BD,
                        Node {
                            addr: 127.0.0.1:34,
                            id: Id(7032...0E34),
                        },
                    ),
                    DistancePair(
                        4326...F7AB,
                        Node {
                            addr: 127.0.0.1:35,
                            id: Id(2871...0F22),
                        },
                    ),
                    DistancePair(
                        3879...B8AF,
                        Node {
                            addr: 127.0.0.1:38,
                            id: Id(532E...4026),
                        },
                    ),
                    DistancePair(
                        1FD8...18AD,
                        Node {
                            addr: 127.0.0.1:39,
                            id: Id(748F...E024),
                        },
                    ),
                ],
            ]
        "#]]
        .assert_debug_eq(&leaves);

        let nodes = (0..100).map(|port| Node::new(format!("127.0.0.1:{port}").parse().unwrap()));
        for node in nodes {
            let pair: DistancePair<Node, 32> = (node, local_node.id()).into();
            let mut leaf = tree.get_leaf_mut(pair.distance());
            let _ = leaf.remove_where(|p| *p == pair);
        }
        let leaves: Vec<_> = tree
            .leaves_iter()
            .map(|leaf| leaf.iter().collect::<Vec<_>>())
            .collect();
        expect!["1"].assert_eq(&leaves.len().to_string());
        expect![[r#"
            [
                [],
            ]
        "#]]
        .assert_debug_eq(&leaves);
    }
}

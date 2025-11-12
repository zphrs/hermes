use std::hint::cold_path;
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
    left: leaf::Leaf<Node, ID_LEN, BUCKET_SIZE>,
    right: Option<Box<Tree<Node, ID_LEN, BUCKET_SIZE>>>,
    // actual cached len might be zero, but recreating a zero length value
    // is a) rare and b) cheap.
    cached_len: AtomicUsize,
    depth: usize,
}

mod bucket;
mod leaf;

pub(crate) use leaf::Leaf;

pub use leaf::Bucket;
use tracing::{instrument, trace};

impl<Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Tree<Node, ID_LEN, BUCKET_SIZE> {
    pub fn new() -> Self {
        Self::new_with_depth(0)
    }

    fn new_with_depth(depth: usize) -> Self {
        Self {
            left: Leaf::new(),
            right: None,
            cached_len: AtomicUsize::new(0),
            depth,
        }
    }

    /// checks if the bucket should split and splits if it should.
    fn maybe_split(&mut self) {
        if self.left.is_full() && self.right.is_none() {
            self.right = Some(Box::new(Tree::new_with_depth(self.depth + 1)));
            let draining: Vec<_> = self.left.drain().collect();
            for pair in draining {
                let mut leaf = self.get_leaf_mut(pair.distance());
                if let Err(_) = leaf.try_insert(pair) {
                    cold_path();
                    unreachable!("should not run out of space since new split was just made")
                }
            }
        }
    }

    fn maybe_split_recursively(&mut self) {
        self.maybe_split();
        if let Some(right) = &mut self.right {
            right.maybe_split_recursively();
        }
    }

    fn maybe_merge_recursively(&mut self) {
        let Some(right) = &mut self.right else {
            return;
        };
        if right.len() + self.left.len() == 0 {
            self.right = None;
        } else {
            right.maybe_merge_recursively();
        }
    }

    pub fn get_leaf(&self, distance: &Distance<ID_LEN>) -> &Leaf<Node, ID_LEN, BUCKET_SIZE> {
        let is_zero = bit_of_array::<ID_LEN>(distance, self.depth);
        match &self.right {
            Some(right) if !is_zero => right.get_leaf(distance),
            _ => &self.left,
        }
    }

    pub fn get_leaf_mut<'a>(
        &'a mut self,
        distance: &Distance<ID_LEN>,
    ) -> LeafMut<'a, Node, ID_LEN, BUCKET_SIZE> {
        self.maybe_split_recursively();
        LeafMut(self, distance.clone())
    }
    #[instrument(skip(self, distance), fields(distance=%distance, depth=self.depth, is_zero=bit_of_array::<ID_LEN>(distance, self.depth)))]
    fn get_leaf_raw_mut(
        &mut self,
        distance: &Distance<ID_LEN>,
    ) -> &mut Leaf<Node, ID_LEN, BUCKET_SIZE> {
        self.maybe_split();

        self.cached_len
            .store(0, std::sync::atomic::Ordering::Release);
        let is_zero = bit_of_array::<ID_LEN>(distance, self.depth);
        match &mut self.right {
            Some(right) if !is_zero => right.get_leaf_raw_mut(distance),
            _ => &mut self.left,
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
        match &self.right {
            Some(right) => {
                let is_zero = bit_of_array::<ID_LEN>(dist, self.depth);
                // figure out which branch is taken next and check if taking the
                // next branch would be less than the splitting factor.
                let next_branch_len = if !is_zero {
                    right.len()
                } else {
                    self.left.len()
                };
                if next_branch_len < length {
                    // should stop here since if we recursed farther we'd end up with an iterator
                    // less than that of length.

                    // should return iter for the one nearer to the target id, then the iter of the
                    // one farther from the target id
                    if !is_zero {
                        Box::new(right.iter().chain(self.left.iter()).take(length))
                    } else {
                        Box::new(self.left.iter().chain(right.iter()).take(length))
                    }
                } else {
                    right.nodes_near(dist, length)
                }
            }
            _ => Box::new(self.left.iter()),
        }
    }

    pub fn nodes_near_mut<const N: usize>(
        &mut self,
        dist: &Distance<N>,
        length: usize,
    ) -> Box<dyn Iterator<Item = &mut DistancePair<Node, ID_LEN>> + '_> {
        match &mut self.right {
            Some(right) => {
                let is_zero = bit_of_array::<ID_LEN>(dist, self.depth);
                // figure out which branch is taken next and check if taking the
                // next branch would be less than the splitting factor.
                let next_branch_len = if !is_zero {
                    right.len()
                } else {
                    self.left.len()
                };
                if next_branch_len < length {
                    // should stop here since if we recursed farther we'd end up with an iterator
                    // less than that of length.

                    // should return iter for the one nearer to the target id, then the iter of the
                    // one farther from the target id
                    if !is_zero {
                        Box::new(self.left.iter_mut().chain(right.iter_mut()).take(length))
                    } else {
                        Box::new(right.iter_mut().chain(self.left.iter_mut()).take(length))
                    }
                } else {
                    right.nodes_near_mut(dist, length)
                }
            }
            _ => Box::new(self.left.iter_mut()),
        }
    }
    /// Iterates by joining recursively, iterating roughly from right (closest)
    /// to left (furthest)
    pub fn iter(&self) -> Box<dyn Iterator<Item = &DistancePair<Node, ID_LEN>> + '_> {
        match &self.right {
            Some(right) => Box::new(right.iter().chain(self.left.iter())),
            None => Box::new(self.left.iter()),
        }
    }
    #[instrument(skip_all, fields(depth=self.depth))]
    pub fn leaves_iter(&self) -> Box<dyn Iterator<Item = &Leaf<Node, ID_LEN, BUCKET_SIZE>> + '_> {
        match &self.right {
            Some(right) => {
                trace!("split");
                Box::new(right.leaves_iter().chain(LeafIterator::new(&self.left)))
            }
            _ => Box::new(LeafIterator::new(&self.left)),
        }
    }

    /// Iterates by joining recursively, iterating roughly from right (closest)
    /// to left (furthest)
    pub fn iter_mut(&mut self) -> Box<dyn Iterator<Item = &mut DistancePair<Node, ID_LEN>> + '_> {
        match &mut self.right {
            Some(right) => Box::new(right.iter_mut().chain(self.left.iter_mut())),
            None => Box::new(self.left.iter_mut()),
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

        let len = match &self.right {
            Some(right) => self.left.len() + right.len(),
            None => self.left.len(),
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
        self.0
            .cached_len
            .store(0, std::sync::atomic::Ordering::Release);
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
    use tracing::trace;
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
        assert!(bit_of_array::<1>(&[0b00000000, 0b10000000], 8));
    }

    #[test]
    #[traced_test]
    pub fn test_lookups() {
        let mut root = Tree::<Node, 32, 2>::new();
        // create a bunch of nodes
        let nodes: Vec<_> = (1..100)
            .map(|i| Node::new(format!("127.0.0.1:{i}").parse().unwrap()))
            .collect();
        let main_node = Node::new(format!("127.0.0.1:0").parse().unwrap());
        let main_id = main_node.id();
        for node in nodes.iter() {
            let mut leaf = root.get_leaf_mut(&(main_id ^ node.id()));
            let _ = leaf.try_insert((node.clone(), main_id));
        }
        let buckets: Vec<_> = root
            .leaves_iter()
            .map(|leaf| {
                let mut out = leaf.iter().collect::<Vec<_>>();
                out.sort();
                out
            })
            .collect();
        trace!(?buckets);
        expect![[r#"
            [
                [
                    DistancePair(
                        03B6...0A61,
                        Node {
                            addr: 127.0.0.1:88,
                            id: Id(137B...ED98),
                        },
                    ),
                ],
                [
                    DistancePair(
                        0579...6BF9,
                        Node {
                            addr: 127.0.0.1:28,
                            id: Id(15B4...9A20),
                        },
                    ),
                    DistancePair(
                        07B3...8B5A,
                        Node {
                            addr: 127.0.0.1:48,
                            id: Id(177E...6CA3),
                        },
                    ),
                ],
                [
                    DistancePair(
                        0A15...AB69,
                        Node {
                            addr: 127.0.0.1:11,
                            id: Id(1AD8...4C90),
                        },
                    ),
                    DistancePair(
                        0C32...228B,
                        Node {
                            addr: 127.0.0.1:13,
                            id: Id(1CFF...C572),
                        },
                    ),
                ],
                [
                    DistancePair(
                        10AE...34A3,
                        Node {
                            addr: 127.0.0.1:82,
                            id: Id(0063...D35A),
                        },
                    ),
                    DistancePair(
                        16CF...561E,
                        Node {
                            addr: 127.0.0.1:8,
                            id: Id(0602...B1E7),
                        },
                    ),
                ],
                [
                    DistancePair(
                        2E66...C6A1,
                        Node {
                            addr: 127.0.0.1:20,
                            id: Id(3EAB...2158),
                        },
                    ),
                    DistancePair(
                        3802...C6AB,
                        Node {
                            addr: 127.0.0.1:17,
                            id: Id(28CF...2152),
                        },
                    ),
                ],
                [
                    DistancePair(
                        631C...B34D,
                        Node {
                            addr: 127.0.0.1:1,
                            id: Id(73D1...54B4),
                        },
                    ),
                    DistancePair(
                        7404...2BAE,
                        Node {
                            addr: 127.0.0.1:2,
                            id: Id(64C9...CC57),
                        },
                    ),
                ],
                [
                    DistancePair(
                        A26D...518A,
                        Node {
                            addr: 127.0.0.1:3,
                            id: Id(B2A0...B673),
                        },
                    ),
                    DistancePair(
                        C276...83E9,
                        Node {
                            addr: 127.0.0.1:4,
                            id: Id(D2BB...6410),
                        },
                    ),
                ],
            ]
        "#]]
        .assert_debug_eq(&buckets);
    }

    #[test]
    #[traced_test]
    pub fn test_tree() {
        let mut tree: Tree<Node, 32, 20> = Tree::new();
        let local_node = Node::new("127.0.0.1:0".parse().unwrap());
        let nodes = (1..100).map(|port| Node::new(format!("127.0.0.1:{port}").parse().unwrap()));
        for node in nodes {
            let pair: DistancePair<Node, 32> = (node, local_node.id()).into();
            let mut leaf = tree.get_leaf_mut(pair.distance());
            let _ = leaf.try_insert(pair);
        }

        let leaves: Vec<_> = tree
            .leaves_iter()
            .map(|leaf| {
                let mut out = leaf.iter().collect::<Vec<_>>();
                out.sort();
                out
            })
            .collect();
        expect!["4"].assert_eq(&leaves.len().to_string());
        expect![[r#"
            [
                [
                    DistancePair(
                        03B6...0A61,
                        Node {
                            addr: 127.0.0.1:88,
                            id: Id(137B...ED98),
                        },
                    ),
                    DistancePair(
                        04B9...33B0,
                        Node {
                            addr: 127.0.0.1:60,
                            id: Id(1474...D449),
                        },
                    ),
                    DistancePair(
                        0579...6BF9,
                        Node {
                            addr: 127.0.0.1:28,
                            id: Id(15B4...9A20),
                        },
                    ),
                    DistancePair(
                        07B3...8B5A,
                        Node {
                            addr: 127.0.0.1:48,
                            id: Id(177E...6CA3),
                        },
                    ),
                    DistancePair(
                        0A15...AB69,
                        Node {
                            addr: 127.0.0.1:11,
                            id: Id(1AD8...4C90),
                        },
                    ),
                    DistancePair(
                        0BBC...9DB6,
                        Node {
                            addr: 127.0.0.1:61,
                            id: Id(1B71...7A4F),
                        },
                    ),
                    DistancePair(
                        0C32...228B,
                        Node {
                            addr: 127.0.0.1:13,
                            id: Id(1CFF...C572),
                        },
                    ),
                    DistancePair(
                        10AE...34A3,
                        Node {
                            addr: 127.0.0.1:82,
                            id: Id(0063...D35A),
                        },
                    ),
                    DistancePair(
                        16CF...561E,
                        Node {
                            addr: 127.0.0.1:8,
                            id: Id(0602...B1E7),
                        },
                    ),
                    DistancePair(
                        1911...A45A,
                        Node {
                            addr: 127.0.0.1:84,
                            id: Id(09DC...43A3),
                        },
                    ),
                ],
                [
                    DistancePair(
                        2384...8939,
                        Node {
                            addr: 127.0.0.1:87,
                            id: Id(3349...6EC0),
                        },
                    ),
                    DistancePair(
                        28BA...F0C5,
                        Node {
                            addr: 127.0.0.1:57,
                            id: Id(3877...173C),
                        },
                    ),
                    DistancePair(
                        2E66...C6A1,
                        Node {
                            addr: 127.0.0.1:20,
                            id: Id(3EAB...2158),
                        },
                    ),
                    DistancePair(
                        2EB9...5637,
                        Node {
                            addr: 127.0.0.1:68,
                            id: Id(3E74...B1CE),
                        },
                    ),
                    DistancePair(
                        2EED...30E6,
                        Node {
                            addr: 127.0.0.1:29,
                            id: Id(3E20...D71F),
                        },
                    ),
                    DistancePair(
                        2FB7...DADF,
                        Node {
                            addr: 127.0.0.1:62,
                            id: Id(3F7A...3D26),
                        },
                    ),
                    DistancePair(
                        3093...90BB,
                        Node {
                            addr: 127.0.0.1:45,
                            id: Id(205E...7742),
                        },
                    ),
                    DistancePair(
                        3345...6909,
                        Node {
                            addr: 127.0.0.1:47,
                            id: Id(2388...8EF0),
                        },
                    ),
                    DistancePair(
                        3643...074F,
                        Node {
                            addr: 127.0.0.1:49,
                            id: Id(268E...E0B6),
                        },
                    ),
                    DistancePair(
                        3749...707E,
                        Node {
                            addr: 127.0.0.1:94,
                            id: Id(2784...9787),
                        },
                    ),
                    DistancePair(
                        37D8...FEB5,
                        Node {
                            addr: 127.0.0.1:67,
                            id: Id(2715...194C),
                        },
                    ),
                    DistancePair(
                        3802...C6AB,
                        Node {
                            addr: 127.0.0.1:17,
                            id: Id(28CF...2152),
                        },
                    ),
                    DistancePair(
                        38BC...E8DB,
                        Node {
                            addr: 127.0.0.1:35,
                            id: Id(2871...0F22),
                        },
                    ),
                    DistancePair(
                        3F8F...2FE9,
                        Node {
                            addr: 127.0.0.1:83,
                            id: Id(2F42...C810),
                        },
                    ),
                ],
                [
                    DistancePair(
                        4074...9450,
                        Node {
                            addr: 127.0.0.1:58,
                            id: Id(50B9...73A9),
                        },
                    ),
                    DistancePair(
                        43E3...A7DF,
                        Node {
                            addr: 127.0.0.1:38,
                            id: Id(532E...4026),
                        },
                    ),
                    DistancePair(
                        4E11...857D,
                        Node {
                            addr: 127.0.0.1:59,
                            id: Id(5EDC...6284),
                        },
                    ),
                    DistancePair(
                        4E25...E707,
                        Node {
                            addr: 127.0.0.1:6,
                            id: Id(5EE8...00FE),
                        },
                    ),
                    DistancePair(
                        5281...92BE,
                        Node {
                            addr: 127.0.0.1:70,
                            id: Id(424C...7547),
                        },
                    ),
                    DistancePair(
                        5BFF...E285,
                        Node {
                            addr: 127.0.0.1:65,
                            id: Id(4B32...057C),
                        },
                    ),
                    DistancePair(
                        5F27...88A9,
                        Node {
                            addr: 127.0.0.1:25,
                            id: Id(4FEA...6F50),
                        },
                    ),
                    DistancePair(
                        60A5...687F,
                        Node {
                            addr: 127.0.0.1:69,
                            id: Id(7068...8F86),
                        },
                    ),
                    DistancePair(
                        60FF...E9CD,
                        Node {
                            addr: 127.0.0.1:34,
                            id: Id(7032...0E34),
                        },
                    ),
                    DistancePair(
                        615D...2C9A,
                        Node {
                            addr: 127.0.0.1:53,
                            id: Id(7190...CB63),
                        },
                    ),
                    DistancePair(
                        631C...B34D,
                        Node {
                            addr: 127.0.0.1:1,
                            id: Id(73D1...54B4),
                        },
                    ),
                    DistancePair(
                        6442...07DD,
                        Node {
                            addr: 127.0.0.1:39,
                            id: Id(748F...E024),
                        },
                    ),
                    DistancePair(
                        6A71...B8A2,
                        Node {
                            addr: 127.0.0.1:14,
                            id: Id(7ABC...5F5B),
                        },
                    ),
                    DistancePair(
                        7015...78B5,
                        Node {
                            addr: 127.0.0.1:23,
                            id: Id(60D8...9F4C),
                        },
                    ),
                    DistancePair(
                        7404...2BAE,
                        Node {
                            addr: 127.0.0.1:2,
                            id: Id(64C9...CC57),
                        },
                    ),
                    DistancePair(
                        75E1...0B2A,
                        Node {
                            addr: 127.0.0.1:5,
                            id: Id(652C...ECD3),
                        },
                    ),
                    DistancePair(
                        7907...75FB,
                        Node {
                            addr: 127.0.0.1:71,
                            id: Id(69CA...9202),
                        },
                    ),
                    DistancePair(
                        7AA7...0BB9,
                        Node {
                            addr: 127.0.0.1:42,
                            id: Id(6A6A...EC40),
                        },
                    ),
                    DistancePair(
                        7B67...89C0,
                        Node {
                            addr: 127.0.0.1:66,
                            id: Id(6BAA...6E39),
                        },
                    ),
                    DistancePair(
                        7F19...F63F,
                        Node {
                            addr: 127.0.0.1:10,
                            id: Id(6FD4...11C6),
                        },
                    ),
                ],
                [
                    DistancePair(
                        8EB8...74C7,
                        Node {
                            addr: 127.0.0.1:36,
                            id: Id(9E75...933E),
                        },
                    ),
                    DistancePair(
                        9393...02D3,
                        Node {
                            addr: 127.0.0.1:12,
                            id: Id(835E...E52A),
                        },
                    ),
                    DistancePair(
                        9AC9...A2CE,
                        Node {
                            addr: 127.0.0.1:33,
                            id: Id(8A04...4537),
                        },
                    ),
                    DistancePair(
                        A156...BABE,
                        Node {
                            addr: 127.0.0.1:18,
                            id: Id(B19B...5D47),
                        },
                    ),
                    DistancePair(
                        A26D...518A,
                        Node {
                            addr: 127.0.0.1:3,
                            id: Id(B2A0...B673),
                        },
                    ),
                    DistancePair(
                        A4F0...2A94,
                        Node {
                            addr: 127.0.0.1:19,
                            id: Id(B43D...CD6D),
                        },
                    ),
                    DistancePair(
                        A8A3...239B,
                        Node {
                            addr: 127.0.0.1:15,
                            id: Id(B86E...C462),
                        },
                    ),
                    DistancePair(
                        B2A5...F708,
                        Node {
                            addr: 127.0.0.1:16,
                            id: Id(A268...10F1),
                        },
                    ),
                    DistancePair(
                        B52D...04AE,
                        Node {
                            addr: 127.0.0.1:22,
                            id: Id(A5E0...E357),
                        },
                    ),
                    DistancePair(
                        BBE2...D1A7,
                        Node {
                            addr: 127.0.0.1:26,
                            id: Id(AB2F...365E),
                        },
                    ),
                    DistancePair(
                        BC60...A778,
                        Node {
                            addr: 127.0.0.1:9,
                            id: Id(ACAD...4081),
                        },
                    ),
                    DistancePair(
                        BF1F...F9D7,
                        Node {
                            addr: 127.0.0.1:37,
                            id: Id(AFD2...1E2E),
                        },
                    ),
                    DistancePair(
                        C276...83E9,
                        Node {
                            addr: 127.0.0.1:4,
                            id: Id(D2BB...6410),
                        },
                    ),
                    DistancePair(
                        D074...5D0F,
                        Node {
                            addr: 127.0.0.1:27,
                            id: Id(C0B9...BAF6),
                        },
                    ),
                    DistancePair(
                        D194...0E57,
                        Node {
                            addr: 127.0.0.1:24,
                            id: Id(C159...E9AE),
                        },
                    ),
                    DistancePair(
                        D42F...8A9E,
                        Node {
                            addr: 127.0.0.1:30,
                            id: Id(C4E2...6D67),
                        },
                    ),
                    DistancePair(
                        E4EC...89E6,
                        Node {
                            addr: 127.0.0.1:7,
                            id: Id(F421...6E1F),
                        },
                    ),
                    DistancePair(
                        EB21...E767,
                        Node {
                            addr: 127.0.0.1:31,
                            id: Id(FBEC...009E),
                        },
                    ),
                    DistancePair(
                        F29E...9F57,
                        Node {
                            addr: 127.0.0.1:21,
                            id: Id(E253...78AE),
                        },
                    ),
                    DistancePair(
                        F67B...18E3,
                        Node {
                            addr: 127.0.0.1:32,
                            id: Id(E6B6...FF1A),
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

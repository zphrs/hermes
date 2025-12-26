use std::fmt::Debug;
use std::ops::{Deref, DerefMut, Index};
use std::sync::atomic::AtomicUsize;

use crate::id::{Distance, DistancePair};
use crate::{HasId, helpers};

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
    // we use 0 to mean that the length is not stored
    // actual cached len might be zero, but recreating a zero length value
    // is a) rare and b) cheap.
    cached_len: AtomicUsize,
    depth: usize,
}

impl<Node: Eq + Debug, const ID_LEN: usize, const BUCKET_SIZE: usize> Debug
    for Tree<Node, ID_LEN, BUCKET_SIZE>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dl = f.debug_list();
        for leaf in self.leaves_iter() {
            dl.entry(&helpers::from_fn(|f| {
                f.debug_list().entries(leaf.iter()).finish()
            }));
        }
        dl.finish()
    }
}

mod leaf;

pub(crate) use leaf::Leaf;

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
                if leaf.try_insert(pair).is_err() {
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
        let nodes: Vec<_> = (1..200)
            .map(|i| Node::new(format!("127.0.0.1:{i}").parse().unwrap()))
            .collect();
        let main_node = Node::new("127.0.0.1:0".parse().unwrap());
        let main_id = main_node.id();
        for node in nodes.iter() {
            let mut leaf = root.get_leaf_mut(&(main_id ^ node.id()));
            let _ = leaf.try_insert((node.clone(), main_id));
        }
        expect![[r#"
            [
                [
                    DistancePair(
                        0091...5DBA,
                        Node {
                            addr: 127.0.0.1:185,
                            id: "Id(A793...2DBC)",
                        },
                    ),
                ],
                [
                    DistancePair(
                        0175...AB73,
                        Node {
                            addr: 127.0.0.1:22,
                            id: "Id(A677...DB75)",
                        },
                    ),
                    DistancePair(
                        01ED...3F62,
                        Node {
                            addr: 127.0.0.1:52,
                            id: "Id(A6EF...4F64)",
                        },
                    ),
                ],
                [
                    DistancePair(
                        028E...49AE,
                        Node {
                            addr: 127.0.0.1:54,
                            id: "Id(A58C...39A8)",
                        },
                    ),
                    DistancePair(
                        02D9...A704,
                        Node {
                            addr: 127.0.0.1:75,
                            id: "Id(A5DB...D702)",
                        },
                    ),
                ],
                [
                    DistancePair(
                        074B...BD9A,
                        Node {
                            addr: 127.0.0.1:12,
                            id: "Id(A049...CD9C)",
                        },
                    ),
                    DistancePair(
                        076C...2614,
                        Node {
                            addr: 127.0.0.1:147,
                            id: "Id(A06E...5612)",
                        },
                    ),
                ],
                [
                    DistancePair(
                        0DB1...6838,
                        Node {
                            addr: 127.0.0.1:10,
                            id: "Id(AAB3...183E)",
                        },
                    ),
                    DistancePair(
                        0CD6...5203,
                        Node {
                            addr: 127.0.0.1:31,
                            id: "Id(ABD4...2205)",
                        },
                    ),
                ],
                [
                    DistancePair(
                        1FE5...B91A,
                        Node {
                            addr: 127.0.0.1:20,
                            id: "Id(B8E7...C91C)",
                        },
                    ),
                    DistancePair(
                        1772...EEEA,
                        Node {
                            addr: 127.0.0.1:36,
                            id: "Id(B070...9EEC)",
                        },
                    ),
                ],
                [
                    DistancePair(
                        212C...B227,
                        Node {
                            addr: 127.0.0.1:1,
                            id: "Id(862E...C221)",
                        },
                    ),
                    DistancePair(
                        2F65...A86B,
                        Node {
                            addr: 127.0.0.1:4,
                            id: "Id(8867...D86D)",
                        },
                    ),
                ],
                [
                    DistancePair(
                        515C...BF1D,
                        Node {
                            addr: 127.0.0.1:2,
                            id: "Id(F65E...CF1B)",
                        },
                    ),
                    DistancePair(
                        5170...3F2A,
                        Node {
                            addr: 127.0.0.1:5,
                            id: "Id(F672...4F2C)",
                        },
                    ),
                ],
                [
                    DistancePair(
                        A96E...72C4,
                        Node {
                            addr: 127.0.0.1:3,
                            id: "Id(0E6C...02C2)",
                        },
                    ),
                    DistancePair(
                        A5EB...4E9B,
                        Node {
                            addr: 127.0.0.1:6,
                            id: "Id(02E9...3E9D)",
                        },
                    ),
                ],
            ]
        "#]]
        .assert_debug_eq(&root);
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
                        0175...AB73,
                        Node {
                            addr: 127.0.0.1:22,
                            id: "Id(A677...DB75)",
                        },
                    ),
                    DistancePair(
                        01ED...3F62,
                        Node {
                            addr: 127.0.0.1:52,
                            id: "Id(A6EF...4F64)",
                        },
                    ),
                    DistancePair(
                        028E...49AE,
                        Node {
                            addr: 127.0.0.1:54,
                            id: "Id(A58C...39A8)",
                        },
                    ),
                    DistancePair(
                        02D9...A704,
                        Node {
                            addr: 127.0.0.1:75,
                            id: "Id(A5DB...D702)",
                        },
                    ),
                    DistancePair(
                        039B...CC84,
                        Node {
                            addr: 127.0.0.1:96,
                            id: "Id(A499...BC82)",
                        },
                    ),
                    DistancePair(
                        074B...BD9A,
                        Node {
                            addr: 127.0.0.1:12,
                            id: "Id(A049...CD9C)",
                        },
                    ),
                    DistancePair(
                        08A5...88A2,
                        Node {
                            addr: 127.0.0.1:43,
                            id: "Id(AFA7...F8A4)",
                        },
                    ),
                    DistancePair(
                        0B6F...C523,
                        Node {
                            addr: 127.0.0.1:95,
                            id: "Id(AC6D...B525)",
                        },
                    ),
                    DistancePair(
                        0C09...B74C,
                        Node {
                            addr: 127.0.0.1:50,
                            id: "Id(AB0B...C74A)",
                        },
                    ),
                    DistancePair(
                        0CD6...5203,
                        Node {
                            addr: 127.0.0.1:31,
                            id: "Id(ABD4...2205)",
                        },
                    ),
                    DistancePair(
                        0DB1...6838,
                        Node {
                            addr: 127.0.0.1:10,
                            id: "Id(AAB3...183E)",
                        },
                    ),
                    DistancePair(
                        0F54...117F,
                        Node {
                            addr: 127.0.0.1:66,
                            id: "Id(A856...6179)",
                        },
                    ),
                    DistancePair(
                        1470...5322,
                        Node {
                            addr: 127.0.0.1:87,
                            id: "Id(B372...2324)",
                        },
                    ),
                    DistancePair(
                        1772...EEEA,
                        Node {
                            addr: 127.0.0.1:36,
                            id: "Id(B070...9EEC)",
                        },
                    ),
                    DistancePair(
                        17E1...8EB8,
                        Node {
                            addr: 127.0.0.1:58,
                            id: "Id(B0E3...FEBE)",
                        },
                    ),
                    DistancePair(
                        1CDF...FA1E,
                        Node {
                            addr: 127.0.0.1:69,
                            id: "Id(BBDD...8A18)",
                        },
                    ),
                    DistancePair(
                        1E8E...9966,
                        Node {
                            addr: 127.0.0.1:71,
                            id: "Id(B98C...E960)",
                        },
                    ),
                    DistancePair(
                        1FE4...4B95,
                        Node {
                            addr: 127.0.0.1:99,
                            id: "Id(B8E6...3B93)",
                        },
                    ),
                    DistancePair(
                        1FE5...B91A,
                        Node {
                            addr: 127.0.0.1:20,
                            id: "Id(B8E7...C91C)",
                        },
                    ),
                ],
                [
                    DistancePair(
                        204C...3362,
                        Node {
                            addr: 127.0.0.1:14,
                            id: "Id(874E...4364)",
                        },
                    ),
                    DistancePair(
                        212C...B227,
                        Node {
                            addr: 127.0.0.1:1,
                            id: "Id(862E...C221)",
                        },
                    ),
                    DistancePair(
                        2595...A747,
                        Node {
                            addr: 127.0.0.1:80,
                            id: "Id(8297...D741)",
                        },
                    ),
                    DistancePair(
                        2863...FC61,
                        Node {
                            addr: 127.0.0.1:83,
                            id: "Id(8F61...8C67)",
                        },
                    ),
                    DistancePair(
                        2A61...C3AE,
                        Node {
                            addr: 127.0.0.1:23,
                            id: "Id(8D63...B3A8)",
                        },
                    ),
                    DistancePair(
                        2C98...EE4D,
                        Node {
                            addr: 127.0.0.1:30,
                            id: "Id(8B9A...9E4B)",
                        },
                    ),
                    DistancePair(
                        2CBB...8361,
                        Node {
                            addr: 127.0.0.1:44,
                            id: "Id(8BB9...F367)",
                        },
                    ),
                    DistancePair(
                        2F65...A86B,
                        Node {
                            addr: 127.0.0.1:4,
                            id: "Id(8867...D86D)",
                        },
                    ),
                    DistancePair(
                        30CD...6177,
                        Node {
                            addr: 127.0.0.1:97,
                            id: "Id(97CF...1171)",
                        },
                    ),
                    DistancePair(
                        341E...2E4A,
                        Node {
                            addr: 127.0.0.1:63,
                            id: "Id(931C...5E4C)",
                        },
                    ),
                    DistancePair(
                        3E0F...E61D,
                        Node {
                            addr: 127.0.0.1:46,
                            id: "Id(990D...961B)",
                        },
                    ),
                ],
                [
                    DistancePair(
                        41C1...A843,
                        Node {
                            addr: 127.0.0.1:13,
                            id: "Id(E6C3...D845)",
                        },
                    ),
                    DistancePair(
                        44FE...325F,
                        Node {
                            addr: 127.0.0.1:60,
                            id: "Id(E3FC...4259)",
                        },
                    ),
                    DistancePair(
                        4665...346D,
                        Node {
                            addr: 127.0.0.1:77,
                            id: "Id(E167...446B)",
                        },
                    ),
                    DistancePair(
                        491E...119E,
                        Node {
                            addr: 127.0.0.1:32,
                            id: "Id(EE1C...6198)",
                        },
                    ),
                    DistancePair(
                        4970...31FB,
                        Node {
                            addr: 127.0.0.1:53,
                            id: "Id(EE72...41FD)",
                        },
                    ),
                    DistancePair(
                        4BC1...4071,
                        Node {
                            addr: 127.0.0.1:76,
                            id: "Id(ECC3...3077)",
                        },
                    ),
                    DistancePair(
                        4ED5...A987,
                        Node {
                            addr: 127.0.0.1:73,
                            id: "Id(E9D7...D981)",
                        },
                    ),
                    DistancePair(
                        515C...BF1D,
                        Node {
                            addr: 127.0.0.1:2,
                            id: "Id(F65E...CF1B)",
                        },
                    ),
                    DistancePair(
                        5170...3F2A,
                        Node {
                            addr: 127.0.0.1:5,
                            id: "Id(F672...4F2C)",
                        },
                    ),
                    DistancePair(
                        556A...B2A1,
                        Node {
                            addr: 127.0.0.1:51,
                            id: "Id(F268...C2A7)",
                        },
                    ),
                    DistancePair(
                        5B57...23A8,
                        Node {
                            addr: 127.0.0.1:42,
                            id: "Id(FC55...53AE)",
                        },
                    ),
                    DistancePair(
                        5CAD...1CC0,
                        Node {
                            addr: 127.0.0.1:70,
                            id: "Id(FBAF...6CC6)",
                        },
                    ),
                    DistancePair(
                        6C29...68AF,
                        Node {
                            addr: 127.0.0.1:7,
                            id: "Id(CB2B...18A9)",
                        },
                    ),
                    DistancePair(
                        6D86...EFC6,
                        Node {
                            addr: 127.0.0.1:39,
                            id: "Id(CA84...9FC0)",
                        },
                    ),
                    DistancePair(
                        6FD8...CF76,
                        Node {
                            addr: 127.0.0.1:48,
                            id: "Id(C8DA...BF70)",
                        },
                    ),
                    DistancePair(
                        70BC...8544,
                        Node {
                            addr: 127.0.0.1:27,
                            id: "Id(D7BE...F542)",
                        },
                    ),
                    DistancePair(
                        75E8...3D22,
                        Node {
                            addr: 127.0.0.1:33,
                            id: "Id(D2EA...4D24)",
                        },
                    ),
                    DistancePair(
                        799B...5BA6,
                        Node {
                            addr: 127.0.0.1:15,
                            id: "Id(DE99...2BA0)",
                        },
                    ),
                    DistancePair(
                        7AA4...9940,
                        Node {
                            addr: 127.0.0.1:25,
                            id: "Id(DDA6...E946)",
                        },
                    ),
                    DistancePair(
                        7FDB...4A2B,
                        Node {
                            addr: 127.0.0.1:72,
                            id: "Id(D8D9...3A2D)",
                        },
                    ),
                ],
                [
                    DistancePair(
                        8A3D...B826,
                        Node {
                            addr: 127.0.0.1:21,
                            id: "Id(2D3F...C820)",
                        },
                    ),
                    DistancePair(
                        91A5...CA5F,
                        Node {
                            addr: 127.0.0.1:9,
                            id: "Id(36A7...BA59)",
                        },
                    ),
                    DistancePair(
                        99A6...21FC,
                        Node {
                            addr: 127.0.0.1:41,
                            id: "Id(3EA4...51FA)",
                        },
                    ),
                    DistancePair(
                        A20D...2D14,
                        Node {
                            addr: 127.0.0.1:16,
                            id: "Id(050F...5D12)",
                        },
                    ),
                    DistancePair(
                        A361...AEAB,
                        Node {
                            addr: 127.0.0.1:40,
                            id: "Id(0463...DEAD)",
                        },
                    ),
                    DistancePair(
                        A5EB...4E9B,
                        Node {
                            addr: 127.0.0.1:6,
                            id: "Id(02E9...3E9D)",
                        },
                    ),
                    DistancePair(
                        A8DB...E38E,
                        Node {
                            addr: 127.0.0.1:34,
                            id: "Id(0FD9...9388)",
                        },
                    ),
                    DistancePair(
                        A96E...72C4,
                        Node {
                            addr: 127.0.0.1:3,
                            id: "Id(0E6C...02C2)",
                        },
                    ),
                    DistancePair(
                        B557...7E26,
                        Node {
                            addr: 127.0.0.1:18,
                            id: "Id(1255...0E20)",
                        },
                    ),
                    DistancePair(
                        BF83...E4D7,
                        Node {
                            addr: 127.0.0.1:35,
                            id: "Id(1881...94D1)",
                        },
                    ),
                    DistancePair(
                        C0D6...1D50,
                        Node {
                            addr: 127.0.0.1:28,
                            id: "Id(67D4...6D56)",
                        },
                    ),
                    DistancePair(
                        C79F...05ED,
                        Node {
                            addr: 127.0.0.1:24,
                            id: "Id(609D...75EB)",
                        },
                    ),
                    DistancePair(
                        CE8C...6110,
                        Node {
                            addr: 127.0.0.1:17,
                            id: "Id(698E...1116)",
                        },
                    ),
                    DistancePair(
                        D7D6...2A75,
                        Node {
                            addr: 127.0.0.1:26,
                            id: "Id(70D4...5A73)",
                        },
                    ),
                    DistancePair(
                        D8BC...016A,
                        Node {
                            addr: 127.0.0.1:29,
                            id: "Id(7FBE...716C)",
                        },
                    ),
                    DistancePair(
                        E6BF...C10B,
                        Node {
                            addr: 127.0.0.1:19,
                            id: "Id(41BD...B10D)",
                        },
                    ),
                    DistancePair(
                        EC47...F08E,
                        Node {
                            addr: 127.0.0.1:37,
                            id: "Id(4B45...8088)",
                        },
                    ),
                    DistancePair(
                        F6CB...DEF6,
                        Node {
                            addr: 127.0.0.1:8,
                            id: "Id(51C9...AEF0)",
                        },
                    ),
                    DistancePair(
                        FD3D...AD2A,
                        Node {
                            addr: 127.0.0.1:11,
                            id: "Id(5A3F...DD2C)",
                        },
                    ),
                    DistancePair(
                        FFF1...4136,
                        Node {
                            addr: 127.0.0.1:38,
                            id: "Id(58F3...3130)",
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
            leaf.remove_where(|p| *p == pair);
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

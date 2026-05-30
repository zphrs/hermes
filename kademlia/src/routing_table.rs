mod bucket_list;
mod leaf;
mod siblings_list;

use std::collections::hash_map;
use std::sync::LazyLock;
use tokio::time::Instant;

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Debug;
use thiserror::Error;

use tracing::instrument;

use crate::routing_table::leaf::Leaf;
use crate::routing_table::siblings_list::SiblingsList;
use crate::{
    HasId, RequestHandler,
    id::{self, Distance, DistancePair},
};

use bucket_list::{BucketList, LeafMut};

const NEARBY_NODES_MULTIP: usize = 5;

static STARTUP_INSTANT: LazyLock<Instant> = LazyLock::new(Instant::now);

pub struct RoutingTable<Node: HasId<ID_LEN>, const ID_LEN: usize, const BUCKET_SIZE: usize = 20> {
    tree: BucketList<Node, ID_LEN, BUCKET_SIZE>,
    // buckets: buckets::Buckets<Node, ID_LEN, BUCKET_SIZE>,
    // the nearest 5*k nodes to me, binary searched & inserted
    // because of the likelihood of having poor rotating codes
    nearest_siblings_list: SiblingsList<Node, ID_LEN>,
    // when each bucket has last been looked_up
    bucket_updated_at: HashMap<usize, tokio::time::Instant>,
}
#[derive(Debug, Error)]
pub enum Error<Node, const ID_LEN: usize> {
    OutOfRange(DistancePair<Node, ID_LEN>),
}

impl<Node: Debug + HasId<ID_LEN> + Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Debug
    for RoutingTable<Node, ID_LEN, BUCKET_SIZE>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoutingTable")
            .field("nearest_siblings_list", &self.nearest_siblings_list)
            .field("tree", &self.tree)
            .field("bucket_updated_at", &self.bucket_updated_at)
            .finish()
    }
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
            tree: BucketList::new(),
            nearest_siblings_list: Default::default(),
            bucket_updated_at: Default::default(),
        }
    }
}

/// to only take the min ownership necessary over bucket_updated_at
/// rather than taking ownership over all of `self`
macro_rules! bucket_updated_at_entry {
    ($owner:expr, $distance:expr) => {
        Self::bucket_entry(&mut $owner.bucket_updated_at, $distance)
    };
}

impl<Node: Eq + Debug + HasId<ID_LEN>, const ID_LEN: usize, const BUCKET_SIZE: usize>
    RoutingTable<Node, ID_LEN, BUCKET_SIZE>
{
    pub fn new() -> Self {
        Self {
            tree: BucketList::new(),
            nearest_siblings_list: Default::default(),
            bucket_updated_at: Default::default(),
        }
    }
    /// Gets a [leaf](Leaf) based on a provided [distance](Distance).
    /// Useful to possibly add a node to a bucket.
    /// Since kademlia recommends potentially pinging each node before inserting
    /// into a specific leaf (when the leaf is full), insertions should be done
    /// directly on the returned leaf.
    pub fn get_leaf_mut<'a>(
        &'a mut self,
        distance: &Distance<ID_LEN>,
    ) -> LeafMut<'a, Node, ID_LEN, BUCKET_SIZE>
    where
        Node: HasId<ID_LEN>,
    {
        bucket_updated_at_entry!(self, distance).or_insert(*STARTUP_INSTANT);
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
    pub(crate) fn sibling_list(&self) -> &SiblingsList<Node, ID_LEN> {
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
        out.extend(self.tree.nodes_near(Distance::ZERO, usize::MAX).cloned());
        out.sort();
        out
    }

    pub fn get_stale_buckets(&self, duration: &std::time::Duration) -> impl Iterator<Item = usize> {
        self.bucket_updated_at
            .iter()
            .filter(|(_, at)| {
                tokio::time::Instant::now().duration_since(**at) < *duration
                    || **at == *STARTUP_INSTANT
            })
            .map(|(idx, _)| *idx)
    }
    /// tries to add nodes to the siblings list.
    /// Returns any nodes which got evicted from the list.
    pub fn maybe_add_nodes_to_siblings_list<DP, Iter: IntoIterator<Item = DP>>(
        &mut self,
        pairs: Iter,
        local_node: &Node,
        handler: &impl RequestHandler<Node, ID_LEN>,
    ) -> impl IntoIterator<Item = DistancePair<Node, ID_LEN>>
    where
        DistancePair<Node, ID_LEN>: From<DP>,
    {
        self.nearest_siblings_list
            .maybe_add_nodes(pairs, local_node, handler)
    }

    pub fn sibling_list_pairs(&self) -> impl Iterator<Item = &DistancePair<Node, ID_LEN>> {
        self.nearest_siblings_list.iter()
    }

    fn bucket_id(distance: &Distance<ID_LEN>) -> usize {
        distance.leading_zeros()
    }

    fn bucket_entry<'a>(
        bucket_updated_at: &'a mut HashMap<usize, Instant>,
        distance: &Distance<ID_LEN>,
    ) -> hash_map::Entry<'a, usize, tokio::time::Instant> {
        bucket_updated_at.entry(Self::bucket_id(distance))
    }

    pub(crate) fn mark_bucket_as_looked_up(&mut self, distance: &Distance<ID_LEN>) {
        bucket_updated_at_entry!(self, distance).insert_entry(Instant::now());
    }

    pub async fn get_unreachable_siblings_list_nodes(
        &self,
        local_node: &Node,
        handler: &impl RequestHandler<Node, ID_LEN>,
    ) -> HashSet<id::Id<ID_LEN>> {
        self.nearest_siblings_list
            .get_unreachable_nodes(local_node, handler)
            .await
    }

    pub fn remove_unreachable_siblings_list_nodes(
        &mut self,
        to_remove_set: HashSet<id::Id<ID_LEN>>,
    ) {
        self.nearest_siblings_list.remove_nodes(to_remove_set);
    }
    /// dist should be distance from the target to the local_node
    #[instrument(skip_all)]
    pub fn nearest_in_sibling_list(
        &self,
        dist: &Distance<ID_LEN>,
        count: usize,
    ) -> std::slice::Iter<'_, DistancePair<Node, ID_LEN>> {
        self.nearest_siblings_list.nearest(dist, count)
    }

    pub fn find_node(
        &self,
        dist: Distance<ID_LEN>,
    ) -> impl Iterator<Item = &DistancePair<Node, ID_LEN>> + '_ {
        self.nearest_in_sibling_list(&dist, BUCKET_SIZE)
            .chain(self.tree.nodes_near(dist, BUCKET_SIZE))
    }

    pub fn find_node_mut(
        &mut self,
        pair: &DistancePair<Node, ID_LEN>,
    ) -> Option<&mut DistancePair<Node, ID_LEN>> {
        self.nearest_siblings_list.node_mut(pair).or_else(|| {
            match self.tree.nodes_near_mut(pair.distance(), 1).next() {
                Some(out) if out == pair => Some(out),
                _ => None,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use expect_test::expect;
    use tracing::debug;
    use tracing_test::traced_test;

    use crate::{HasId, RoutingTable, node::Node};

    struct RpcHandler;
    impl crate::RequestHandler<Node, 32> for RpcHandler {
        async fn ping(&self, _from: &Node, _node: &Node) -> bool {
            unimplemented!()
        }

        async fn find_node(&self, _from: &Node, _to: &Node, _address: &crate::Id<32>) -> Vec<Node> {
            unimplemented!()
        }
    }
    #[test]
    #[traced_test]
    pub fn siblings() {
        let mut table: RoutingTable<_, _, 1> = RoutingTable::new();
        // generate 1k nodes in a loop
        let self_node = Node::new(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)));
        for _ in 0..10 {
            let nodes = (1..1001).map(|port| {
                (
                    Node::new(format!("127.0.0.1:{port}").parse().unwrap()),
                    self_node.id(),
                )
            });
            table.maybe_add_nodes_to_siblings_list(
                nodes,
                &Node::new(([0, 0, 0, 0], 1000).into()),
                &RpcHandler,
            );
        }
        expect![[r#"
            SiblingsList(
                [
                    DistancePair(
                        0035...8F8D,
                        Node {
                            addr: 127.0.0.1:833,
                            id: "Id(90C2...7074)",
                        },
                    ),
                    DistancePair(
                        0040...AA40,
                        Node {
                            addr: 127.0.0.1:351,
                            id: "Id(90B7...55B9)",
                        },
                    ),
                    DistancePair(
                        005B...50E1,
                        Node {
                            addr: 127.0.0.1:221,
                            id: "Id(90AC...AF18)",
                        },
                    ),
                    DistancePair(
                        00BA...83A4,
                        Node {
                            addr: 127.0.0.1:345,
                            id: "Id(904D...7C5D)",
                        },
                    ),
                    DistancePair(
                        00D1...156A,
                        Node {
                            addr: 127.0.0.1:789,
                            id: "Id(9026...EA93)",
                        },
                    ),
                    DistancePair(
                        00E9...3572,
                        Node {
                            addr: 127.0.0.1:945,
                            id: "Id(901E...CA8B)",
                        },
                    ),
                    DistancePair(
                        010B...3881,
                        Node {
                            addr: 127.0.0.1:839,
                            id: "Id(91FC...C778)",
                        },
                    ),
                    DistancePair(
                        010C...A02E,
                        Node {
                            addr: 127.0.0.1:734,
                            id: "Id(91FB...5FD7)",
                        },
                    ),
                    DistancePair(
                        0125...1873,
                        Node {
                            addr: 127.0.0.1:541,
                            id: "Id(91D2...E78A)",
                        },
                    ),
                    DistancePair(
                        0128...EC06,
                        Node {
                            addr: 127.0.0.1:330,
                            id: "Id(91DF...13FF)",
                        },
                    ),
                    DistancePair(
                        01C1...A354,
                        Node {
                            addr: 127.0.0.1:411,
                            id: "Id(9136...5CAD)",
                        },
                    ),
                    DistancePair(
                        01D2...F749,
                        Node {
                            addr: 127.0.0.1:222,
                            id: "Id(9125...08B0)",
                        },
                    ),
                    DistancePair(
                        01D6...B507,
                        Node {
                            addr: 127.0.0.1:305,
                            id: "Id(9121...4AFE)",
                        },
                    ),
                    DistancePair(
                        0245...8314,
                        Node {
                            addr: 127.0.0.1:978,
                            id: "Id(92B2...7CED)",
                        },
                    ),
                    DistancePair(
                        0341...AA33,
                        Node {
                            addr: 127.0.0.1:228,
                            id: "Id(93B6...55CA)",
                        },
                    ),
                    DistancePair(
                        0343...37A5,
                        Node {
                            addr: 127.0.0.1:313,
                            id: "Id(93B4...C85C)",
                        },
                    ),
                    DistancePair(
                        03A6...842B,
                        Node {
                            addr: 127.0.0.1:944,
                            id: "Id(9351...7BD2)",
                        },
                    ),
                    DistancePair(
                        03BE...0D94,
                        Node {
                            addr: 127.0.0.1:115,
                            id: "Id(9349...F26D)",
                        },
                    ),
                    DistancePair(
                        03DB...CB09,
                        Node {
                            addr: 127.0.0.1:899,
                            id: "Id(932C...34F0)",
                        },
                    ),
                    DistancePair(
                        03EB...A1B5,
                        Node {
                            addr: 127.0.0.1:63,
                            id: "Id(931C...5E4C)",
                        },
                    ),
                    DistancePair(
                        03F8...4980,
                        Node {
                            addr: 127.0.0.1:200,
                            id: "Id(930F...B679)",
                        },
                    ),
                    DistancePair(
                        04DE...76B0,
                        Node {
                            addr: 127.0.0.1:680,
                            id: "Id(9429...8949)",
                        },
                    ),
                    DistancePair(
                        0520...36A9,
                        Node {
                            addr: 127.0.0.1:969,
                            id: "Id(95D7...C950)",
                        },
                    ),
                    DistancePair(
                        054C...42D4,
                        Node {
                            addr: 127.0.0.1:101,
                            id: "Id(95BB...BD2D)",
                        },
                    ),
                    DistancePair(
                        055E...F7C2,
                        Node {
                            addr: 127.0.0.1:532,
                            id: "Id(95A9...083B)",
                        },
                    ),
                    DistancePair(
                        0560...FD3A,
                        Node {
                            addr: 127.0.0.1:614,
                            id: "Id(9597...02C3)",
                        },
                    ),
                    DistancePair(
                        0590...737C,
                        Node {
                            addr: 127.0.0.1:836,
                            id: "Id(9567...8C85)",
                        },
                    ),
                    DistancePair(
                        0641...0517,
                        Node {
                            addr: 127.0.0.1:323,
                            id: "Id(96B6...FAEE)",
                        },
                    ),
                    DistancePair(
                        06BF...059B,
                        Node {
                            addr: 127.0.0.1:436,
                            id: "Id(9648...FA62)",
                        },
                    ),
                    DistancePair(
                        0738...EE88,
                        Node {
                            addr: 127.0.0.1:97,
                            id: "Id(97CF...1171)",
                        },
                    ),
                    DistancePair(
                        076B...F94B,
                        Node {
                            addr: 127.0.0.1:883,
                            id: "Id(979C...06B2)",
                        },
                    ),
                    DistancePair(
                        0797...7C3C,
                        Node {
                            addr: 127.0.0.1:184,
                            id: "Id(9760...83C5)",
                        },
                    ),
                    DistancePair(
                        07A3...68D5,
                        Node {
                            addr: 127.0.0.1:961,
                            id: "Id(9754...972C)",
                        },
                    ),
                    DistancePair(
                        07C0...1DD0,
                        Node {
                            addr: 127.0.0.1:525,
                            id: "Id(9737...E229)",
                        },
                    ),
                    DistancePair(
                        0857...FB57,
                        Node {
                            addr: 127.0.0.1:374,
                            id: "Id(98A0...04AE)",
                        },
                    ),
                    DistancePair(
                        0859...2E1B,
                        Node {
                            addr: 127.0.0.1:136,
                            id: "Id(98AE...D1E2)",
                        },
                    ),
                    DistancePair(
                        086B...7DA0,
                        Node {
                            addr: 127.0.0.1:487,
                            id: "Id(989C...8259)",
                        },
                    ),
                    DistancePair(
                        0928...B1E4,
                        Node {
                            addr: 127.0.0.1:481,
                            id: "Id(99DF...4E1D)",
                        },
                    ),
                    DistancePair(
                        09D4...AE94,
                        Node {
                            addr: 127.0.0.1:853,
                            id: "Id(9923...516D)",
                        },
                    ),
                    DistancePair(
                        09F2...B8A1,
                        Node {
                            addr: 127.0.0.1:471,
                            id: "Id(9905...4758)",
                        },
                    ),
                    DistancePair(
                        09FA...69E2,
                        Node {
                            addr: 127.0.0.1:46,
                            id: "Id(990D...961B)",
                        },
                    ),
                    DistancePair(
                        0A1A...0858,
                        Node {
                            addr: 127.0.0.1:476,
                            id: "Id(9AED...F7A1)",
                        },
                    ),
                    DistancePair(
                        0A50...7025,
                        Node {
                            addr: 127.0.0.1:165,
                            id: "Id(9AA7...8FDC)",
                        },
                    ),
                    DistancePair(
                        0A5D...2B6D,
                        Node {
                            addr: 127.0.0.1:253,
                            id: "Id(9AAA...D494)",
                        },
                    ),
                    DistancePair(
                        0AF9...86AE,
                        Node {
                            addr: 127.0.0.1:504,
                            id: "Id(9A0E...7957)",
                        },
                    ),
                    DistancePair(
                        0B05...054E,
                        Node {
                            addr: 127.0.0.1:250,
                            id: "Id(9BF2...FAB7)",
                        },
                    ),
                    DistancePair(
                        0B59...E1B0,
                        Node {
                            addr: 127.0.0.1:819,
                            id: "Id(9BAE...1E49)",
                        },
                    ),
                    DistancePair(
                        0B77...C5D4,
                        Node {
                            addr: 127.0.0.1:890,
                            id: "Id(9B80...3A2D)",
                        },
                    ),
                    DistancePair(
                        0B8F...11E1,
                        Node {
                            addr: 127.0.0.1:118,
                            id: "Id(9B78...EE18)",
                        },
                    ),
                    DistancePair(
                        0C1D...445A,
                        Node {
                            addr: 127.0.0.1:999,
                            id: "Id(9CEA...BBA3)",
                        },
                    ),
                    DistancePair(
                        0C2A...3611,
                        Node {
                            addr: 127.0.0.1:847,
                            id: "Id(9CDD...C9E8)",
                        },
                    ),
                    DistancePair(
                        0C2E...F6B7,
                        Node {
                            addr: 127.0.0.1:434,
                            id: "Id(9CD9...094E)",
                        },
                    ),
                    DistancePair(
                        0C63...7920,
                        Node {
                            addr: 127.0.0.1:686,
                            id: "Id(9C94...86D9)",
                        },
                    ),
                    DistancePair(
                        0D09...A08E,
                        Node {
                            addr: 127.0.0.1:171,
                            id: "Id(9DFE...5F77)",
                        },
                    ),
                    DistancePair(
                        0F17...C502,
                        Node {
                            addr: 127.0.0.1:361,
                            id: "Id(9FE0...3AFB)",
                        },
                    ),
                    DistancePair(
                        0FBE...5B97,
                        Node {
                            addr: 127.0.0.1:220,
                            id: "Id(9F49...A46E)",
                        },
                    ),
                    DistancePair(
                        0FCC...FCD1,
                        Node {
                            addr: 127.0.0.1:235,
                            id: "Id(9F3B...0328)",
                        },
                    ),
                    DistancePair(
                        0FD4...570D,
                        Node {
                            addr: 127.0.0.1:777,
                            id: "Id(9F23...A8F4)",
                        },
                    ),
                    DistancePair(
                        0FE2...B5F6,
                        Node {
                            addr: 127.0.0.1:129,
                            id: "Id(9F15...4A0F)",
                        },
                    ),
                    DistancePair(
                        1022...707A,
                        Node {
                            addr: 127.0.0.1:437,
                            id: "Id(80D5...8F83)",
                        },
                    ),
                    DistancePair(
                        1055...4728,
                        Node {
                            addr: 127.0.0.1:564,
                            id: "Id(80A2...B8D1)",
                        },
                    ),
                    DistancePair(
                        10D8...EE9C,
                        Node {
                            addr: 127.0.0.1:846,
                            id: "Id(802F...1165)",
                        },
                    ),
                    DistancePair(
                        10DF...1C84,
                        Node {
                            addr: 127.0.0.1:496,
                            id: "Id(8028...E37D)",
                        },
                    ),
                    DistancePair(
                        1145...2446,
                        Node {
                            addr: 127.0.0.1:178,
                            id: "Id(81B2...DBBF)",
                        },
                    ),
                    DistancePair(
                        114B...C8EC,
                        Node {
                            addr: 127.0.0.1:168,
                            id: "Id(81BC...3715)",
                        },
                    ),
                    DistancePair(
                        114F...6B1E,
                        Node {
                            addr: 127.0.0.1:413,
                            id: "Id(81B8...94E7)",
                        },
                    ),
                    DistancePair(
                        117E...8D52,
                        Node {
                            addr: 127.0.0.1:741,
                            id: "Id(8189...72AB)",
                        },
                    ),
                    DistancePair(
                        1231...EE3A,
                        Node {
                            addr: 127.0.0.1:815,
                            id: "Id(82C6...11C3)",
                        },
                    ),
                    DistancePair(
                        1240...1044,
                        Node {
                            addr: 127.0.0.1:309,
                            id: "Id(82B7...EFBD)",
                        },
                    ),
                    DistancePair(
                        1258...341A,
                        Node {
                            addr: 127.0.0.1:288,
                            id: "Id(82AF...CBE3)",
                        },
                    ),
                    DistancePair(
                        1260...28B8,
                        Node {
                            addr: 127.0.0.1:80,
                            id: "Id(8297...D741)",
                        },
                    ),
                    DistancePair(
                        127B...E123,
                        Node {
                            addr: 127.0.0.1:962,
                            id: "Id(828C...1EDA)",
                        },
                    ),
                    DistancePair(
                        1285...9B2A,
                        Node {
                            addr: 127.0.0.1:639,
                            id: "Id(8272...64D3)",
                        },
                    ),
                    DistancePair(
                        12DD...CBEC,
                        Node {
                            addr: 127.0.0.1:108,
                            id: "Id(822A...3415)",
                        },
                    ),
                    DistancePair(
                        136B...69BE,
                        Node {
                            addr: 127.0.0.1:988,
                            id: "Id(839C...9647)",
                        },
                    ),
                    DistancePair(
                        1442...6D81,
                        Node {
                            addr: 127.0.0.1:940,
                            id: "Id(84B5...9278)",
                        },
                    ),
                    DistancePair(
                        14A3...C730,
                        Node {
                            addr: 127.0.0.1:670,
                            id: "Id(8454...38C9)",
                        },
                    ),
                    DistancePair(
                        14E4...DD52,
                        Node {
                            addr: 127.0.0.1:192,
                            id: "Id(8413...22AB)",
                        },
                    ),
                    DistancePair(
                        1533...0C35,
                        Node {
                            addr: 127.0.0.1:937,
                            id: "Id(85C4...F3CC)",
                        },
                    ),
                    DistancePair(
                        15C8...B8DC,
                        Node {
                            addr: 127.0.0.1:577,
                            id: "Id(853F...4725)",
                        },
                    ),
                    DistancePair(
                        15DB...9566,
                        Node {
                            addr: 127.0.0.1:470,
                            id: "Id(852C...6A9F)",
                        },
                    ),
                    DistancePair(
                        169B...F4AB,
                        Node {
                            addr: 127.0.0.1:739,
                            id: "Id(866C...0B52)",
                        },
                    ),
                    DistancePair(
                        16A6...23C8,
                        Node {
                            addr: 127.0.0.1:517,
                            id: "Id(8651...DC31)",
                        },
                    ),
                    DistancePair(
                        16D9...3DD8,
                        Node {
                            addr: 127.0.0.1:1,
                            id: "Id(862E...C221)",
                        },
                    ),
                    DistancePair(
                        173A...411F,
                        Node {
                            addr: 127.0.0.1:857,
                            id: "Id(87CD...BEE6)",
                        },
                    ),
                    DistancePair(
                        17A0...E4E9,
                        Node {
                            addr: 127.0.0.1:243,
                            id: "Id(8757...1B10)",
                        },
                    ),
                    DistancePair(
                        17B9...BC9D,
                        Node {
                            addr: 127.0.0.1:14,
                            id: "Id(874E...4364)",
                        },
                    ),
                    DistancePair(
                        17C6...5CDE,
                        Node {
                            addr: 127.0.0.1:667,
                            id: "Id(8731...A327)",
                        },
                    ),
                    DistancePair(
                        1890...2794,
                        Node {
                            addr: 127.0.0.1:4,
                            id: "Id(8867...D86D)",
                        },
                    ),
                    DistancePair(
                        18DA...B764,
                        Node {
                            addr: 127.0.0.1:865,
                            id: "Id(882D...489D)",
                        },
                    ),
                    DistancePair(
                        1982...8827,
                        Node {
                            addr: 127.0.0.1:242,
                            id: "Id(8975...77DE)",
                        },
                    ),
                    DistancePair(
                        19A1...719A,
                        Node {
                            addr: 127.0.0.1:757,
                            id: "Id(8956...8E63)",
                        },
                    ),
                    DistancePair(
                        19A7...B65E,
                        Node {
                            addr: 127.0.0.1:197,
                            id: "Id(8950...49A7)",
                        },
                    ),
                    DistancePair(
                        1A19...F657,
                        Node {
                            addr: 127.0.0.1:954,
                            id: "Id(8AEE...09AE)",
                        },
                    ),
                    DistancePair(
                        1A77...F908,
                        Node {
                            addr: 127.0.0.1:694,
                            id: "Id(8A80...06F1)",
                        },
                    ),
                    DistancePair(
                        1A85...2835,
                        Node {
                            addr: 127.0.0.1:407,
                            id: "Id(8A72...D7CC)",
                        },
                    ),
                    DistancePair(
                        1A8E...395B,
                        Node {
                            addr: 127.0.0.1:562,
                            id: "Id(8A79...C6A2)",
                        },
                    ),
                    DistancePair(
                        1AA9...E795,
                        Node {
                            addr: 127.0.0.1:627,
                            id: "Id(8A5E...186C)",
                        },
                    ),
                    DistancePair(
                        1AAD...4BB2,
                        Node {
                            addr: 127.0.0.1:276,
                            id: "Id(8A5A...B44B)",
                        },
                    ),
                    DistancePair(
                        1AAD...6A3A,
                        Node {
                            addr: 127.0.0.1:117,
                            id: "Id(8A5A...95C3)",
                        },
                    ),
                ],
            )
        "#]]
        .assert_debug_eq(table.sibling_list());
        debug!("{:#?}", table.sibling_list());
    }
}

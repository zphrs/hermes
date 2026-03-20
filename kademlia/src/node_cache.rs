mod kademlia;

pub use kademlia::NodeCache as KadNodeCache;

use std::ops::Deref;

use crate::{Distance, HasId, Id, RequestHandler};

pub trait Cullable<Client, Node, const ID_LEN: usize> {
    type CullSet;

    fn find_removal_candidates(
        &self,
        nodes: impl Iterator<Item = Node>,
        handler: &impl RequestHandler<Client, Node, ID_LEN>,
    ) -> impl Future<Output = Self::CullSet>;

    fn remove_candidates(&mut self, candidates: Self::CullSet);
}

pub trait NodeCache<Client, Node: HasId<ID_LEN>, const ID_LEN: usize>:
    Cullable<Client, Node, ID_LEN>
{
    fn add_nodes(&mut self, nodes: impl IntoIterator<Item = Node>);

    fn on_node_lookup(&mut self, _id: &Id<ID_LEN>) {}

    // finds the nearest known nodes to a specific address
    fn nearby_nodes<'a>(&'a self, address: &Id<ID_LEN>) -> impl Iterator<Item = &'a Node>
    where
        Node: 'a;
}

pub trait MaintnenceLookupAddrs<
    Client,
    Node: HasId<ID_LEN>,
    Cache: NodeCache<Client, Node, ID_LEN>,
    const ID_LEN: usize,
>
{
    /// bootstrap address lookups; which addresses to look up to bootstrap
    /// knowing many nodes in the network
    fn bootstrap_addrs(
        &self,
        cache: impl Deref<Target = Cache>,
    ) -> impl IntoIterator<Item = crate::Id<ID_LEN>>;
}

impl<Client, Node: HasId<ID_LEN>, Cache: NodeCache<Client, Node, ID_LEN>, const ID_LEN: usize>
    MaintnenceLookupAddrs<Client, Node, Cache, ID_LEN> for Node
{
    // 2.3: To join the network, a node u must have a contact to an already
    // participating node w. u inserts w into the appropriate k-bucket. u then
    // performs a node lookup for its own node ID. Finally, u refreshes all
    // k-buckets further away than its closest neighbor. During the refreshes, u
    // both populates its own k-buckets and inserts itself into other nodes'
    // k-buckets as necessary.
    //
    // 2.3: Refreshing means picking a random ID in the bucket's range and
    // performing a node search for that ID. Run internally when joining network
    fn bootstrap_addrs(
        &self,
        cache: impl Deref<Target = Cache>,
    ) -> impl IntoIterator<Item = crate::Id<ID_LEN>> {
        let f = |shift_by| shifted_target_id(self.id(), shift_by);
        let Some(nearest_node) = cache.nearby_nodes(self.id()).next() else {
            return (0..0).map(f);
        };
        let dist_to_nearest = self.id().xor_distance(nearest_node.id());
        let lz_count = dist_to_nearest.leading_zeros();

        let shift_by = ID_LEN * 8 - lz_count;

        fn shifted_target_id<const ID_LEN: usize>(
            local: &Id<ID_LEN>,
            shift_by: usize,
        ) -> Id<ID_LEN> {
            let randomized_distance_from_self: [u8; ID_LEN] = rand::random();
            let shifted_dist: Distance<ID_LEN> =
                Distance::new(randomized_distance_from_self) >> (ID_LEN * 8 - shift_by);

            let next_bucket_addr = &shifted_dist;
            (next_bucket_addr ^ local).clone()
        }
        (shift_by..(shift_by + lz_count)).map(f)
    }
}

use crate::Id;

// trait to actually handle any network egress on behalf of the RPC manager.
pub trait RequestHandler<Node, const ID_LEN: usize> {
    /// returns whether the node is online
    fn ping(&self, from: &Node, node: &Node) -> impl std::future::Future<Output = bool> + Send;
    /// tells a node to return the BUCKET_SIZE nearest known & alive nodes to an
    /// address
    fn find_node(
        &self,
        from: &Node,
        to: &Node,
        address: &Id<ID_LEN>,
    ) -> impl std::future::Future<Output = Vec<Node>> + Send;
}

pub trait HasId<const N: usize> {
    fn id(&self) -> &Id<N>;
    const BITS: usize = N / 8;
}

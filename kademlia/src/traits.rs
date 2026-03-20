use crate::Id;

// trait to actually handle any network egress on behalf of the RPC manager.
pub trait RequestHandler<Client, Node, const ID_LEN: usize> {
    /// returns whether the node is online
    fn ping(&self, from: &Client, node: &Node) -> impl std::future::Future<Output = bool>;
    /// tells a node to return the BUCKET_SIZE nearest known & alive nodes to an
    /// address
    fn find_node(
        &self,
        from: &Client,
        to: &Node,
        address: &Id<ID_LEN>,
    ) -> impl std::future::Future<Output = Vec<Node>> + Send;
}

pub trait HasId<const N: usize> {
    fn id(&self) -> &Id<N>;
    const BITS: usize = N / 8;
}

pub trait HasServerId<Server: HasId<N>, const N: usize> {
    fn server_id(&self) -> Id<N>;
}

impl<Server: HasId<N>, const N: usize> HasServerId<Server, N> for Server {
    fn server_id(&self) -> Id<N> {
        self.id().clone()
    }
}

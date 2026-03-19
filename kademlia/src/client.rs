use crate::{HasId, Id};

/// A way to interface with server nodes
pub struct LookupClient<
    Client: HasId<ID_LEN>,
    Node: HasId<ID_LEN>,
    Handler: AnonymousRequestHandler<Client, Node, ID_LEN>,
    const ID_LEN: usize,
> {
    handler: Handler,
    client: Client,
    // arbitrary choice of 128
    known_nodes: arrayvec::ArrayVec<Node, 128>,
}
pub trait AnonymousRequestHandler<Client, Node, const ID_LEN: usize> {
    /// returns whether the node is online
    fn ping(&self, from: &Client, node: &Node) -> impl std::future::Future<Output = bool> + Send;
    /// tells a node to return the BUCKET_SIZE nearest known & alive nodes to an
    /// address
    fn find_node(
        &self,
        from: &Client,
        to: &Node,
        address: &Id<ID_LEN>,
    ) -> impl std::future::Future<Output = Vec<Node>> + Send;
}

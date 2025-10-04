use crate::id::Id;

pub enum ValueResponse<Node, Value> {
    Value(Value),
    Nodes(Vec<Node>),
}

// trait to actually handle any network egress on behalf of the RPC manager.
pub trait RequestHandler<Node, Value, const ID_LEN: usize> {
    /// returns whether the node is online
    async fn ping(&self, node: &Node) -> bool;
    /// tells a node to return the BUCKET_SIZE nearest known & alive nodes to an
    /// address
    async fn find_node(&self, node: &Node, address: &Id<ID_LEN>) -> Vec<Node>;
    /// finds a value based on an address or a list of nodes
    async fn find_value(&self, address: &Id<ID_LEN>) -> ValueResponse<Node, Value>;
}

pub trait LocalValueHandler<Node, Value, const ID_LEN: usize> {
    async fn get(id: &Id<ID_LEN>) -> Value;
    async fn put(id: Id<ID_LEN>, value: Value);
}

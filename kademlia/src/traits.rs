use crate::Id;

/// The status of the node. See BitTorrent's [BEP
/// 5](https://bittorrent.org/beps/bep_0005.html#routing-table) for how
/// BitTorrent determines the status of nodes.
pub enum NodeStatus {
    /// we have recently heard from this node, no need to worry about possibly
    /// removing it
    Good = 0,
    /// we don't know its status/haven't heard from it in a while/ever, should
    /// probably try to ping it to renew its status.
    Unknown = 1,
    /// means that the node is dead, should be removed (only when space is
    /// needed)
    Bad = 2,
}

// trait to actually handle any network egress on behalf of the RPC manager.
pub trait RequestHandler<Node, const ID_LEN: usize> {
    /// returns whether the node is online
    ///
    /// Should return whether a node is "good", meaning it has been heard from
    /// recently. Used to avoid pinging if NodeStatus is good or bad. Will fall
    /// back to performing a ping if a node's status is Unknown (the
    /// default implementation always returns [`Unknown`](NodeStatus::Unknown)).
    ///
    /// Primarily used to optimize non-async insertions which cannot await any
    /// asynchronous calls, like [`ping()`](RequestHandler::ping).
    ///
    /// A node with a [`Good`](NodeStatus::Good) status will not be considered
    /// for removal to make space for a new node and will not trigger a ping
    /// call. A node with a [`Bad`](NodeStatus::Bad) status will be immediately
    /// removed without bothering to ping.
    ///
    /// Can also be used as a hook on which to spawn threads to ping the
    /// specified node in a non-blocking manner to refresh its good status, if
    /// the return result is [`Unknown`](NodeStatus::Unknown).
    fn node_status(&self, from: &Node, node: &Node) -> NodeStatus {
        // throw away in the def so auto generated impls and docs will not have
        // underscores in front of the parameters
        let _ = (from, node);
        NodeStatus::Unknown
    }
    /// returns whether the node is online. Should probably try to ping at least
    /// twice to avoid incorrectly removing a long-standing node due to a
    /// spurious failure.
    ///
    /// This should also update the node being pinged to allow for future calls
    /// to [`is_good()`](RequestHandler::is_good) to resolve to either
    /// [`Good`](NodeStatus::Good) or [`Bad`](NodeStatus::Bad) rather than
    /// [`Unknown`](NodeStatus::Unknown) to avoid unnecessary future calls and
    /// to allow optimizations when performing non-blocking insertions.
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

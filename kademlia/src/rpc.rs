use std::marker::PhantomData;

use futures::{future::join_all, prelude::*, stream::FuturesUnordered};
// sync is runtime agnostic;
// see https://docs.rs/tokio/latest/tokio/sync/index.html#runtime-compatibility
use tokio::sync::{RwLock, RwLockWriteGuard};

use crate::{
    HasId, RoutingTable,
    id::{Distance, DistancePair, Id},
    routing_table,
    traits::RequestHandler,
};

/// key is assumed to be an Id<ID_LEN>
pub struct RpcManager<
    Node: Eq + HasId<ID_LEN>,
    Value,
    Handler: RequestHandler<Node, Value, ID_LEN>,
    const ID_LEN: usize,
    const BUCKET_SIZE: usize,
> {
    _value: PhantomData<Value>,
    handler: Handler,
    routing_table: RwLock<RoutingTable<Node, ID_LEN, BUCKET_SIZE>>,
    local_addr: Id<ID_LEN>,
}

impl<
    Node: Eq + HasId<ID_LEN>,
    Value,
    Handler: RequestHandler<Node, Value, ID_LEN>,
    const ID_LEN: usize,
    const BUCKET_SIZE: usize,
> RpcManager<Node, Value, Handler, ID_LEN, BUCKET_SIZE>
{
    pub fn new(handler: Handler, local_addr: Id<ID_LEN>) -> Self {
        let routing_table = RoutingTable::new();

        Self {
            _value: PhantomData,
            handler,
            routing_table: routing_table.into(),
            local_addr,
        }
    }

    fn local_addr(&self) -> &Id<ID_LEN> {
        &self.local_addr
    }

    pub async fn add_node(&self, node: Node) {
        let mut write_lock = self.routing_table.write().await;

        let Some(pair) = self.maybe_add_node_to_siblings_list(node, &mut write_lock) else {
            return;
        };
        // didn't add to siblings list
        drop(write_lock);
        let Some(to_remove) = self.get_removal_candidates(&pair).await else {
            // leaf was full and no removal candidates were found.
            return;
        };

        let mut write_lock = self.routing_table.write().await;

        self.remove_and_insert(to_remove, pair, &mut write_lock)
            .await;
    }

    async fn remove_and_insert(
        &self,
        to_remove: Vec<Distance<ID_LEN>>,
        pair: DistancePair<Node, ID_LEN>,
        lock: &mut RwLockWriteGuard<'_, RoutingTable<Node, ID_LEN, BUCKET_SIZE>>,
    ) {
        let leaf = lock.get_leaf_mut(pair.distance());
        leaf.remove_where(|pair| to_remove.contains(pair.distance()));
        // since we either made room or returned early, we can safely
        // assume that the leaf likely has room when doing this insertion.
        let _ = leaf.try_insert(pair);
    }

    async fn get_removal_candidates(
        &self,
        pair: &DistancePair<Node, ID_LEN>,
    ) -> Option<Vec<Distance<ID_LEN>>> {
        let routing_table = self.routing_table.read().await;
        let leaf = routing_table.get_leaf(pair.distance());
        let leaf_len = leaf.len();
        // probably smart to batch this

        if leaf.is_full() {
            let mut failed_to_ping: Vec<Distance<_>> = vec![];
            {
                let unordered_futures = FuturesUnordered::new();
                for pair in leaf.iter() {
                    unordered_futures.push(async move {
                        (self.handler.ping(pair.node()).await, pair.distance())
                    });
                }
                let mut unordered_futures_chunks = unordered_futures.ready_chunks(leaf_len);
                while let Some(finished_pings) = unordered_futures_chunks.next().await {
                    for (ping_succeeded, distance) in finished_pings {
                        if !ping_succeeded {
                            failed_to_ping.push(distance.clone());
                        }
                    }
                    if failed_to_ping.len() != 0 {
                        // we've freed up at least some nodes
                        break;
                    }
                }
            }
            if failed_to_ping.len() == 0 {
                // no room was freed up, all nodes in bucket were online
                return None;
            }
            Some(failed_to_ping)
        } else {
            Some(vec![])
        }
    }

    fn maybe_add_node_to_siblings_list(
        &self,
        node: Node,
        table_lock: &mut RwLockWriteGuard<'_, RoutingTable<Node, ID_LEN, BUCKET_SIZE>>,
    ) -> Option<DistancePair<Node, ID_LEN>> {
        let pair: DistancePair<Node, ID_LEN> = (node, self.local_addr()).into();
        let Some(pair) = table_lock.maybe_add_node_to_siblings_list(pair) else {
            return None;
        };
        Some(pair)
    }

    /// pipelines the three stages for the nodes which allows all to complete
    /// their write, read, write stages in synchronicity when inserting.
    pub async fn add_nodes(&self, nodes: impl IntoIterator<Item = Node>) {
        let mut write_lock = self.routing_table.write().await;
        let leftover: Vec<_> = nodes
            .into_iter()
            .map(|node| self.maybe_add_node_to_siblings_list(node, &mut write_lock))
            .filter_map(|node| node)
            .collect();

        drop(write_lock);

        let unordered = FuturesUnordered::new();
        for pair in leftover {
            unordered.push(async { (self.get_removal_candidates(&pair).await, pair) });
        }
        let unordered = unordered.filter_map(async |v| {
            if let Some(to_remove) = v.0 {
                Some((to_remove, v.1))
            } else {
                None
            }
        });

        let to_removes: Vec<_> = unordered.collect().await;
        let mut write_lock = self.routing_table.write().await;
        for (to_remove, pair) in to_removes {
            self.remove_and_insert(to_remove, pair, &mut write_lock)
                .await;
        }
    }
}

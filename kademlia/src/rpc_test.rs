use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use sha2::Digest as _;
use tracing_test::traced_test;

use crate::{HasId, Id, RequestHandler};

const ID_LEN: usize = 256;
const BUCKET_SIZE: usize = 20;

#[derive(Clone, PartialEq, Eq)]
struct Node {
    addr: String,
    id: Id<ID_LEN>,
}

impl Node {
    pub fn new(addr: impl AsRef<str>) -> Self {
        let mut hasher = sha2::Sha256::new();
        hasher.update(addr.as_ref().as_bytes());
        let hash: [u8; 32] = *hasher.finalize().as_array().unwrap();
        let id = Id::from(hash);
        Self {
            addr: addr.as_ref().into(),
            id,
        }
    }
}

impl std::hash::Hash for Node {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.addr.hash(state);
    }
}

impl HasId<ID_LEN> for Node {
    fn id(&self) -> &Id<ID_LEN> {
        &self.id
    }
}

impl Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Node")
            .field(&self.addr)
            .field(&format!("{}", self.id))
            .finish()
    }
}

#[derive(Clone)]
struct RpcAdapter {
    network: NetworkState,
    num_rpcs: Arc<AtomicUsize>,
}

impl RequestHandler<Node, ID_LEN> for RpcAdapter {
    #[tracing::instrument(skip_all, fields(from=_from.addr))]
    async fn ping(&self, _from: &Node, to: &Node) -> bool {
        self.num_rpcs.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(num_rpcs = self.num_rpcs.load(Ordering::Relaxed));
        // tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        self.network.manager(to).is_some()
    }

    #[tracing::instrument(skip_all, fields(from=from.addr))]
    async fn find_node(&self, from: &Node, to: &Node, id: &Id<ID_LEN>) -> Vec<Node> {
        self.num_rpcs.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(num_rpcs = self.num_rpcs.load(Ordering::Relaxed));
        let Some(manager) = self.network.manager(to) else {
            return vec![];
        };
        // tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        manager.find_node(from.clone(), id).await
    }
}

type RpcManager = crate::RpcManager<Node, RpcAdapter, ID_LEN, BUCKET_SIZE>;

struct ManagerState {
    manager: RpcManager,
}

#[derive(Clone, Default)]
struct NetworkState {
    nodes: Arc<RwLock<HashMap<String, ManagerState>>>,
}

impl NetworkState {
    fn manager(&self, node: &Node) -> Option<RpcManager> {
        let nodes = self.nodes.read().unwrap();
        nodes.get(&node.addr).map(|s| &s.manager).cloned()
    }

    fn add_node(&self, node: Node) -> RpcManager {
        let adapter = RpcAdapter {
            network: self.clone(),
            num_rpcs: Arc::new(AtomicUsize::new(0)),
        };
        let manager = RpcManager::new(adapter, node.clone());
        let state = ManagerState {
            manager: manager.clone(),
        };
        let mut nodes = self.nodes.write().unwrap();
        nodes.insert(node.addr.to_string(), state);
        manager
    }
}

#[derive(Clone, Default)]
struct NodeAllocator {
    count: Arc<Mutex<usize>>,
}

impl NodeAllocator {
    fn new_node(&self) -> Node {
        let mut count = self.count.lock().unwrap();
        let addr = format!("node-{count}");
        *count += 1;
        Node::new(addr)
    }
}

async fn bootstrap_and_join(manager: &RpcManager, bootstrap_nodes: impl IntoIterator<Item = Node>) {
    manager.add_nodes(bootstrap_nodes).await;
    manager.join_network().await;
}

#[tokio::test(flavor = "multi_thread")]
#[traced_test]
async fn find_exact_peer_through_bootstrap() {
    let net = NetworkState::default();
    let a = NodeAllocator::default();

    let bootstrap_node = a.new_node();
    bootstrap_and_join(&net.add_node(bootstrap_node.clone()), []).await;

    let my_node = a.new_node();
    let my_manager = net.add_node(my_node.clone());
    bootstrap_and_join(&my_manager, [bootstrap_node.clone()]).await;

    let peer_node = a.new_node();
    bootstrap_and_join(
        &net.add_node(peer_node.clone()),
        vec![bootstrap_node.clone()],
    )
    .await;

    let result = my_manager.find_node(my_node.clone(), peer_node.id()).await;
    assert_ne!(result.len(), 0);
    assert_eq!(result[0], peer_node);
}

#[tokio::test(flavor = "multi_thread")]
#[traced_test]
async fn find_exact_peer_boostrap_self() {
    let net = NetworkState::default();
    let a = NodeAllocator::default();

    let my_node = a.new_node();
    let my_manager = net.add_node(my_node.clone());
    bootstrap_and_join(&my_manager, []).await;

    let peer_node = a.new_node();
    let peer_manager = net.add_node(peer_node.clone());
    bootstrap_and_join(&peer_manager, vec![my_node.clone()]).await;

    let result = my_manager.find_node(my_node.clone(), peer_node.id()).await;
    assert_ne!(result.len(), 0);
    assert_eq!(result[0], peer_node);
}

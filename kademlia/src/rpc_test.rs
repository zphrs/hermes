use std::{
    collections::HashMap,
    fmt::Debug,
    iter,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use futures::future;
use rand::seq::IndexedRandom as _;
use sha2::Digest as _;
use tracing::{instrument::WithSubscriber as _, subscriber::NoSubscriber};
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
    connected: bool,
}

#[derive(Clone, Default)]
struct NetworkState {
    nodes: Arc<RwLock<HashMap<String, ManagerState>>>,
}

impl NetworkState {
    fn manager(&self, node: &Node) -> Option<RpcManager> {
        let nodes = self.nodes.read().unwrap();
        let Some(ManagerState {
            manager,
            connected: true,
        }) = nodes.get(&node.addr)
        else {
            return None;
        };
        Some(manager.clone())
    }

    fn add_node(&self, node: Node) -> RpcManager {
        let adapter = RpcAdapter {
            network: self.clone(),
            num_rpcs: Arc::new(AtomicUsize::new(0)),
        };
        let manager = RpcManager::new(adapter, node.clone());
        let state = ManagerState {
            manager: manager.clone(),
            connected: true,
        };
        let mut nodes = self.nodes.write().unwrap();
        nodes.insert(node.addr.to_string(), state);
        manager
    }

    #[allow(dead_code)]
    fn set_connected(&self, node: &Node, connected: bool) {
        let mut nodes = self.nodes.write().unwrap();
        if let Some(state) = nodes.get_mut(&node.addr) {
            state.connected = connected;
        }
    }

    fn all_nodes(&self) -> Vec<Node> {
        self.nodes
            .read()
            .unwrap()
            .keys()
            .map(|addr| Node::new(addr))
            .collect()
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

    let result = my_manager.node_lookup(peer_node.id()).await;
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

    let result = my_manager.node_lookup(peer_node.id()).await;
    assert_ne!(result.len(), 0);
    assert_eq!(result[0], peer_node);

    let result = peer_manager.node_lookup(my_node.id()).await;
    assert_ne!(result.len(), 0);
    assert_eq!(result[0], my_node);
}

async fn create_large_network(net: NetworkState, a: NodeAllocator, size: usize) -> Vec<Node> {
    let mut nodes = Vec::new();

    let bootstrap_node = a.new_node();
    bootstrap_and_join(&net.add_node(bootstrap_node.clone()), []).await;
    nodes.push(bootstrap_node.clone());

    let bootstrap_nodes: Vec<_> = iter::repeat_with(|| a.new_node())
        .take((size as f64).log2() as usize + 1)
        .collect();
    nodes.extend(bootstrap_nodes.iter().cloned());
    let bootstrap_nodes = Arc::new(bootstrap_nodes);
    future::join_all(bootstrap_nodes.iter().cloned().map(async |node| {
        let manager = net.add_node(node.clone());
        bootstrap_and_join(&manager, vec![bootstrap_node.clone()])
            .with_subscriber(NoSubscriber::new())
            .await;
    }))
    .await;

    let mut tasks = tokio::task::JoinSet::new();
    for i in 0..size {
        tasks.spawn({
            let net = net.clone();
            let a = a.clone();
            let bootstrap_node = bootstrap_nodes[i % bootstrap_nodes.len()].clone();
            async move {
                let node = a.new_node();
                let manager = net.add_node(node.clone());
                bootstrap_and_join(&manager, vec![bootstrap_node.clone()])
                    .with_subscriber(NoSubscriber::new())
                    .await;
                node
            }
        });
    }
    nodes.extend(tasks.join_all().await);
    nodes
}

#[tokio::test(flavor = "multi_thread")]
#[traced_test]
async fn find_peer_large_network() {
    let num_nodes = 100;
    let num_trials = 10;

    let net = NetworkState::default();
    let a = NodeAllocator::default();
    let rng = &mut rand::rng();

    let nodes = create_large_network(net.clone(), a.clone(), num_nodes).await;

    /// Experiment: bootstrap using random node and find a random peer
    async fn bootstrap_and_find_peer(
        net: &NetworkState,
        a: &NodeAllocator,
        bootstrap_node: Node,
        target_node: Node,
    ) {
        let my_node = a.new_node();
        let my_manager = net.add_node(my_node.clone());
        bootstrap_and_join(&my_manager, vec![bootstrap_node])
            .with_subscriber(NoSubscriber::new())
            .await;

        let result = my_manager.node_lookup(target_node.id()).await;
        assert_ne!(result.len(), 0);
        assert_eq!(result[0], target_node);
    }

    future::join_all(
        iter::repeat_with(|| {
            (
                nodes.choose(rng).unwrap().clone(),
                nodes.choose(rng).unwrap().clone(),
            )
        })
        .map(async |(bootstrap_node, target_node)| {
            bootstrap_and_find_peer(&net, &a, bootstrap_node, target_node)
        })
        .take(num_trials),
    )
    .await;
}

fn find_closest_node(nodes: &[Node], target: &Node) -> Node {
    nodes
        .iter()
        .filter(|&n| n.id() != target.id())
        .min_by_key(|n| n.id() ^ target.id())
        .unwrap()
        .clone()
}

#[tokio::test(flavor = "multi_thread")]
#[traced_test]
async fn find_nonexistent_peer_returns_closest() {
    let num_nodes = 100;
    let num_trials = 20;

    let net = NetworkState::default();
    let a = NodeAllocator::default();

    let nodes = create_large_network(net.clone(), a.clone(), num_nodes).await;

    let my_node = a.new_node();
    let my_manager = net.add_node(my_node.clone());
    bootstrap_and_join(&my_manager, vec![nodes[0].clone()]).await;

    let nodes = net.all_nodes();

    // Experiment: arbitrarily generate a node and try to find it
    for _ in 0..num_trials {
        let target_node = a.new_node();

        let next_closest_node = find_closest_node(&nodes, &target_node);
        let result = my_manager.node_lookup(target_node.id()).await;
        assert_ne!(result.len(), 0);
        assert_eq!(result[0], next_closest_node);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[traced_test]
async fn find_nonexistent_peer_small_network() {
    let net = NetworkState::default();
    let a = NodeAllocator::default();

    let bootstrap_node = a.new_node();
    bootstrap_and_join(&net.add_node(bootstrap_node.clone()), []).await;

    let my_node = a.new_node();
    let my_manager = net.add_node(my_node.clone());
    bootstrap_and_join(&my_manager, vec![bootstrap_node.clone()]).await;

    let nodes = net.all_nodes();

    for _ in 0..10 {
        let target_node = a.new_node();

        let next_closest_node = find_closest_node(&nodes, &target_node);
        let result = my_manager.node_lookup(target_node.id()).await;
        assert_ne!(result.len(), 0);
        assert_eq!(result[0], next_closest_node);
    }
}

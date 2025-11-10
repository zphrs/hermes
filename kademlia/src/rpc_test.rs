use std::{
    collections::HashMap,
    fmt::Debug,
    iter,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use futures::{StreamExt as _, future};
use futures_time::time::Duration;
use rand::seq::IndexedRandom as _;
use sha2::Digest as _;
use tokio_stream::wrappers::ReadDirStream;
use tracing::{debug, instrument::WithSubscriber as _, subscriber::NoSubscriber, trace};
use tracing_test::traced_test;

use crate::{DistancePair, HasId, Id, RequestHandler};

const ID_LEN: usize = 32;
const BUCKET_SIZE: usize = 3;

#[derive(Clone, PartialEq, Eq)]
struct Node {
    addr: String,
    id: Id<ID_LEN>,
}

impl<'de> serde::Deserialize<'de> for Node {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let addr = String::deserialize(deserializer)?;
        Ok(Self::new(addr))
    }
}

impl serde::Serialize for Node {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.addr.serialize(serializer)
    }
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
        write!(f, "Node({:?}, {})", self.addr, self.id)
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

    async fn to_serializable(&self) -> Vec<(Node, Vec<Node>)> {
        future::join_all(
            self.nodes
                .read()
                .unwrap()
                .values()
                .map(|state| state.manager.to_parts()),
        )
        .await
    }

    async fn load_data(&self, data: Vec<(Node, Vec<Node>)>) {
        for (node, peers) in data {
            let addr = node.addr.clone();
            let handler = RpcAdapter {
                network: self.clone(),
                num_rpcs: Arc::new(AtomicUsize::new(0)),
            };
            let manager = RpcManager::from_parts_unchecked(handler, node, peers).await;
            let state = ManagerState {
                manager: manager.clone(),
                connected: true,
            };
            let mut nodes = self.nodes.write().unwrap();
            nodes.insert(addr.to_string(), state);
        }
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

    // let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..size {
        // tasks.spawn({
        let net = net.clone();
        let a = a.clone();
        let bootstrap_node = bootstrap_node.clone();
        let new_node = async move {
            let node = a.new_node();
            let manager = net.add_node(node.clone());
            bootstrap_and_join(&manager, vec![bootstrap_node.clone()])
                .with_subscriber(NoSubscriber::new())
                .await;
            node
        }
        .await;
        nodes.push(new_node);
        // });
    }
    // nodes.extend(tasks.join_all().await);

    let mut tasks = tokio::task::JoinSet::new();
    for node in nodes.iter() {
        let manager = net.manager(node).unwrap();
        tasks.spawn(async move {
            manager
                .refresh_stale_buckets(&Duration::new(0, 0))
                .with_subscriber(NoSubscriber::new())
                .await;
        });
    }

    tasks.join_all().await;
    nodes
}

#[tokio::test(flavor = "multi_thread")]
#[traced_test]
async fn find_peer_large_network() {
    let num_nodes = 1000;
    let num_trials = 1000;

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
    let num_nodes = 1000;
    let num_trials = 1;

    let net = NetworkState::default();
    let a = NodeAllocator::default();

    let nodes = create_large_network(net.clone(), a.clone(), num_nodes).await;

    let my_node = nodes[0].clone();
    let my_manager = net.manager(&my_node).unwrap();
    // my_manager.join_network().await;
    // bootstrap_and_join(&my_manager, vec![nodes[0].clone()]).await;

    let nodes = net.all_nodes();

    // Experiment: arbitrarily generate a node and try to find it
    for _ in 0..num_trials {
        let target_node = Node::new("node-1017");
        let mut closest_nodes: Vec<_> = nodes
            .iter()
            .cloned()
            .map::<DistancePair<_, _>, _>(|n| (n, target_node.id()).into())
            .collect();
        closest_nodes.sort();
        closest_nodes.truncate(BUCKET_SIZE * 5);
        let next_closest_node = find_closest_node(&nodes, &target_node);
        let next_closest_manager = net.manager(&next_closest_node).unwrap();
        // next_closest_manager.join_network().await;
        let result = my_manager.node_lookup(target_node.id()).await;
        let result_pairs: Vec<DistancePair<_, _>> = result
            .iter()
            .cloned()
            .map(|n| (n, target_node.id()).into())
            .collect();
        trace!(?result_pairs, ?closest_nodes);
        let dists_from_target = [
            DistancePair::from((result[0].clone(), target_node.id())),
            DistancePair::from((next_closest_node.clone(), target_node.id())),
        ];

        trace!(?dists_from_target);
        trace!(?target_node);
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

fn network_files_dir(test_name: &str, num_nodes: usize) -> String {
    format!(
        "{}/target/kademlia-nets/{test_name}-{num_nodes}",
        env!("CARGO_MANIFEST_DIR")
    )
}

async fn save_network(net: &NetworkState, test_name: &str, num_nodes: usize) {
    let dir = network_files_dir(test_name, num_nodes);
    debug!(path = dir, "saving network to disk");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let data = net.to_serializable().await;
    let mut tasks = tokio::task::JoinSet::new();
    for entry in data {
        let file_path = format!("{dir}/{}", entry.0.addr);
        tasks.spawn(async move {
            let data = postcard::to_stdvec(&entry).unwrap();
            tokio::fs::write(file_path, data).await.unwrap();
        });
    }
    tasks.join_all().await;
}

#[tracing::instrument]
async fn load_network(test_name: &str, num_nodes: usize) -> NetworkState {
    let dir = network_files_dir(test_name, num_nodes);

    if !tokio::fs::try_exists(&dir).await.unwrap() {
        debug!("network dir not found");

        let net = NetworkState::default();
        let a = NodeAllocator::default();
        create_large_network(net.clone(), a.clone(), num_nodes)
            .with_subscriber(NoSubscriber::new())
            .await;
        save_network(&net, test_name, num_nodes).await;

        return net;
    }

    debug!("loading network from disk");

    let net = NetworkState::default();

    let mut read_dir = ReadDirStream::new(tokio::fs::read_dir(&dir).await.unwrap());
    let mut tasks = tokio::task::JoinSet::new();
    while let Some(entry) = read_dir.next().await {
        let path = entry.unwrap().path();
        tasks.spawn(async move {
            let data = tokio::fs::read(path).await.unwrap();
            let entry: (Node, Vec<Node>) = postcard::from_bytes(&data).unwrap();
            entry
        });
    }

    let data = tasks.join_all().await;
    net.load_data(data).await;

    net
}

#[tokio::test(flavor = "multi_thread")]
#[traced_test]
async fn large_network_find_exact_node() {
    let net = load_network("10000_nodes", 10000).await;

    let nodes = net.all_nodes();

    let my_node = Node::new("my-node");
    let my_manager = net.add_node(my_node.clone());
    bootstrap_and_join(
        &my_manager,
        vec![nodes.choose(&mut rand::rng()).unwrap().clone()],
    )
    .with_subscriber(NoSubscriber::new())
    .await;

    future::join_all(
        iter::repeat_with(async || {
            let target_node = nodes.choose(&mut rand::rng()).unwrap().clone();
            let result = my_manager.node_lookup(target_node.id()).await;
            assert_ne!(result.len(), 0);
            assert_eq!(result[0], target_node);
        })
        .take(10),
    )
    .await;
}

use std::{
    collections::HashMap,
    fmt::Debug,
    iter, panic,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

use futures::{StreamExt as _, future};
use rand::seq::IndexedRandom as _;
use sha2::Digest as _;
use tokio::task::JoinSet;
use tokio_stream::wrappers::ReadDirStream;
use tracing::{debug, instrument::WithSubscriber as _, subscriber::NoSubscriber, trace};
use tracing_test::traced_test;

use crate::{DistancePair, HasId, Id, RequestHandler};

const ID_LEN: usize = 32;
const BUCKET_SIZE: usize = 20;

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

        let id = Id::from(hasher.finalize().as_slice());
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
        // tracing::debug!(num_rpcs = self.num_rpcs.load(Ordering::Relaxed));
        // tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        self.network.manager(to).is_some()
    }

    #[tracing::instrument(skip_all, fields(from=from.addr))]
    async fn find_node(&self, from: &Node, to: &Node, id: &Id<ID_LEN>) -> Vec<Node> {
        self.num_rpcs.fetch_add(1, Ordering::Relaxed);
        // tracing::debug!(num_rpcs = self.num_rpcs.load(Ordering::Relaxed));
        let Some(manager) = self.network.manager(to) else {
            return vec![];
        };
        // tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        manager.find_node(from.clone(), id).await
    }
}

type RpcManager = crate::RpcManager<Node, RpcAdapter, ID_LEN, BUCKET_SIZE>;

#[derive(Clone)]
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
        self.nodes.read().unwrap().keys().map(Node::new).collect()
    }

    async fn to_serializable(&self) -> Vec<(Node, Vec<Node>)> {
        let values: Vec<_> = {
            let guard = self.nodes.read().unwrap();
            guard.values().cloned().collect()
        };
        future::join_all(
            values
                .into_iter()
                .map(async move |state| state.manager.to_parts().await),
        )
        .await
    }

    async fn load_data(&self, data: Vec<(Node, Vec<Node>)>) {
        let mut js = JoinSet::new();
        for (node, peers) in data {
            let addr = node.addr.clone();
            let handler = RpcAdapter {
                network: self.clone(),
                num_rpcs: Arc::new(AtomicUsize::new(0)),
            };
            js.spawn(async move {
                let manager = RpcManager::from_parts_unchecked(handler, node, peers).await;
                let state = ManagerState {
                    manager,
                    connected: true,
                };
                (addr, state)
            });
        }
        let states = js.join_all().await;
        trace!("made all managers");
        let mut nodes = self.nodes.write().unwrap();
        for (addr, state) in states {
            nodes.insert(addr, state);
        }
    }
}

#[derive(Clone, Default)]
struct NodeAllocator {
    count: Arc<Mutex<usize>>,
}

impl NodeAllocator {
    fn new(count: usize) -> Self {
        Self {
            count: Arc::new(count.into()),
        }
    }
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

    trace!("bootstrapped first node");

    let mut bootstrap_nodes: Vec<_> = (0..size.ilog2()).map(|_| a.new_node()).collect();
    let mut js = JoinSet::new();
    for node in bootstrap_nodes.iter().cloned() {
        let bootstrap_node = bootstrap_node.clone();
        let net = net.clone();
        js.spawn(async move {
            bootstrap_and_join(&net.add_node(node), [bootstrap_node.clone()])
                .with_subscriber(NoSubscriber::new())
                .await
        });
    }

    js.join_all().await;

    trace!("bootstrapped bootstrap nodes");

    let leftover_nodes = size - (size.ilog2() as usize) - 1;

    const SPAWN_CHUNK_SIZE: usize = 10_000;

    for outer in 0..(leftover_nodes / SPAWN_CHUNK_SIZE) {
        let mut tasks = tokio::task::JoinSet::new();
        for inner in 0..SPAWN_CHUNK_SIZE {
            let i = outer * SPAWN_CHUNK_SIZE + inner;
            tasks.spawn({
                let net = net.clone();
                let a = a.clone();
                let bootstrap_node = bootstrap_nodes[i % bootstrap_nodes.len()].clone();
                let node = a.new_node();
                async move {
                    let manager = net.add_node(node.clone());
                    bootstrap_and_join(&manager, vec![bootstrap_node])
                        .with_subscriber(NoSubscriber::new())
                        .await;
                    node
                }
            });
        }
        trace!("spawned {outer}/{}", leftover_nodes / SPAWN_CHUNK_SIZE);
        nodes.append(&mut tasks.join_all().await);
    }
    trace!("spawned all bootstraps");
    nodes.append(&mut bootstrap_nodes);

    let mut tasks = tokio::task::JoinSet::new();
    for node in nodes.iter() {
        let manager = net.manager(node).unwrap();
        tasks.spawn(
            async move {
                manager.refresh_stale_buckets(&Duration::ZERO).await;
            }
            .with_subscriber(NoSubscriber::new()),
        );
    }

    tasks.join_all().await;
    trace!("finished spawning all nodes");
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
            bootstrap_and_find_peer(&net, &a, bootstrap_node, target_node).await
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

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[traced_test]
async fn find_nonexistent_peer_returns_closest() {
    let num_nodes = 100_000;
    let num_trials = 100_000;

    let net = load_network("find_nonexistent_peer_returns_closest", num_nodes).await;
    trace!("fully loaded network");
    let a = NodeAllocator::new(num_nodes + 1);

    let nodes = net.all_nodes();

    // my_manager.join_network().await;
    // bootstrap_and_join(&my_manager, vec![nodes[0].clone()]).await;

    // Experiment: arbitrarily generate a node and try to find it
    let parallelization_factor = 20_000;
    'outer: for i in 0..(num_trials / parallelization_factor) {
        let mut js = JoinSet::new();
        for _ in 0..parallelization_factor {
            let target_node = a.new_node();
            let net = net.clone();
            let nodes = nodes.clone();

            js.spawn(
                async move {
                    let mut closest_nodes: Vec<_> = nodes
                        .iter()
                        .cloned()
                        .map::<DistancePair<_, _>, _>(|n| (n, target_node.id()).into())
                        .collect();
                    closest_nodes.sort();
                    closest_nodes.truncate(BUCKET_SIZE * 5);
                    let next_closest_node = find_closest_node(&nodes, &target_node);
                    let my_manager = net
                        .manager(nodes.choose(&mut rand::rng()).unwrap())
                        .unwrap();
                    let result = my_manager.node_lookup(target_node.id()).await;
                    // let next_closest_manager = net.manager(&next_closest_node).unwrap();
                    let result_pairs: Vec<DistancePair<_, _>> = result
                        .iter()
                        .cloned()
                        .map(|n| (n, target_node.id()).into())
                        .collect();
                    let dists_from_target = [
                        DistancePair::from((result[0].clone(), target_node.id())),
                        DistancePair::from((next_closest_node.clone(), target_node.id())),
                    ];

                    trace!(?result_pairs);
                    let result_table_str = {
                        let result_manager = net.manager(&result[0]).unwrap();
                        format!("{:?}", result_manager.routing_table.read().await)
                    };

                    // assert_eq!(result[0], next_closest_node);

                    (
                        result[0].clone(),
                        next_closest_node,
                        dists_from_target,
                        target_node,
                        result_pairs,
                        result_table_str,
                    )
                }
                .with_subscriber(NoSubscriber::new()),
            );
        }
        while let Some(res) = js.join_next().await {
            match res {
                Err(err) => {
                    if err.is_panic() {
                        panic::resume_unwind(err.into_panic())
                    }
                }
                Ok(v) => {
                    let (
                        result,
                        next_closest_node,
                        dists_from_target,
                        target_node,
                        result_pairs,
                        result_table_str,
                    ) = v;
                    if result != next_closest_node {
                        trace!(
                            "Error querying for {:?}. Found {:?}, closest {:?}",
                            target_node, result, next_closest_node
                        );
                        trace!("{}", result_table_str.as_str());
                        trace!(?dists_from_target, ?result_pairs);
                        break 'outer;
                    }
                }
            }
        }
        js.abort_all();
        trace!("{i}/{} done", num_trials / parallelization_factor);
        // sleep(Duration::from_secs(2)).await;
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
        create_large_network(net.clone(), a.clone(), num_nodes).await;
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
    trace!("loaded data from disk");
    net.load_data(data).await;
    trace!("loaded disk data into the network");
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

use std::{cell::RefCell, net::IpAddr, rc::Rc, time::Duration};

use dens::{
    OsMock,
    net::ip,
    sim::{MachineIntoRef, MachineRef, Sim, machine::HasMachineId},
};

use kademlia::HasId;

use rand::seq::SliceRandom;
use shared_schema::{EarthNode, SkyNode, earth_node::EarthId};
use tracing::{Instrument, info, info_span, instrument::WithSubscriber, trace, warn};

use crate::{
    client::SkyClient, get_system_time::since_epoch, quinn_transport::Transport, server::SkyServer,
    tokio_uptime,
};

#[test]
fn kad() {
    let sim = Sim::new();
    // Create a `fmt` subscriber that uses our custom event format, and set it
    // as the default.

    sim.enter_runtime(|| {
        let net = Sim::add_machine(ip::Network::new_private_class_c());

        trace!("bootstrapped first node");
        let num_inited = Rc::new(RefCell::new(0));
        let mut bootstrap_servers: Vec<MachineRef<OsMock>> = vec![];
        let mut machine_addresses: Vec<SkyNode> = vec![];
        // join servers in 10 rounds of 2^i
        let mut spawned_so_far = 0;
        for i in 0..7 {
            let round_size = 2usize.pow(i);
            let mut prev_bootstrap_addresses = machine_addresses.clone();
            for _ in 0..round_size {
                let (sample_of_machine_addresses, _) =
                    prev_bootstrap_addresses.partial_shuffle(&mut rand::rng(), 10);
                let new_server = create_sky_node(
                    net,
                    sample_of_machine_addresses.into(),
                    spawned_so_far,
                    num_inited.clone(),
                );
                let addr: IpAddr = new_server.get().borrow().public_ips().unwrap().0.into();
                machine_addresses.push(addr.into());
                bootstrap_servers.push(new_server);
            }
            spawned_so_far += round_size;
        }
        // spawn fixed sized rounds (finished bootstrapping)
        for _ in 0..2 {
            let round_size = 100;
            let mut prev_bootstrap_addresses = machine_addresses.clone();
            for _ in 0..round_size {
                let (sample_of_machine_addresses, _) =
                    prev_bootstrap_addresses.partial_shuffle(&mut rand::rng(), 10);
                let new_server = create_sky_node(
                    net,
                    sample_of_machine_addresses.into(),
                    spawned_so_far,
                    num_inited.clone(),
                );
                let addr: IpAddr = new_server.get().borrow().public_ips().unwrap().0.into();
                machine_addresses.push(addr.into());
                bootstrap_servers.push(new_server);
            }
            spawned_so_far += round_size;
        }
        // spawn clients
        let mut clients = Vec::new();
        for _ in 0..50 {
            let (sample_of_servers, _) = bootstrap_servers.partial_shuffle(&mut rand::rng(), 10);

            let client = create_client(
                net,
                sample_of_servers.into(),
                machine_addresses.clone(),
                EarthId::ZERO,
                num_inited.clone(),
            );
            clients.push(client);
        }

        trace!("spawned {spawned_so_far} nodes");

        match Sim::run_until_idle(|| clients.iter()) {
            Ok(_) => {}
            Err(e) => {
                println!("{e}");
                panic!()
            }
        }
    })
}

fn create_client(
    net: MachineRef<ip::Network>,
    servers: Vec<MachineRef<OsMock>>,
    all_server_addrs: Vec<SkyNode>,
    nearby: EarthId,
    num_inited: Rc<RefCell<usize>>,
) -> MachineRef<OsMock> {
    let client = OsMock::new(move || {
        let servers = servers.clone();
        let nearby = nearby.clone();
        let all_server_addrs = all_server_addrs.clone();
        let num_inited = num_inited.clone();
        async move {
            let _dir = std::fs::create_dir_all("logs").unwrap();
            let val = Sim::get_current_machine_ref::<OsMock>();
            let num = val.id();
            let output_file = std::fs::File::create(format!("logs/client_{num}.log")).unwrap();
            async move {
                trace!("running client");
                let tp = Transport::client().await?;
                while *num_inited.borrow() < all_server_addrs.len() {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    info!(
                        "{}/{} spawned!",
                        *num_inited.borrow(),
                        all_server_addrs.len()
                    )
                }
                // make sure there's time for nodes to refresh their buckets
                info!("All spawned! sleeping for a bit");
                tokio::time::sleep(Duration::from_secs(60 * 11)).await;
                info!("Finished sleeping");
                warn!("maybe randomly choose between v4 and v6 addresses");
                let server_addrs: Vec<SkyNode> = servers
                    .iter()
                    .enumerate()
                    .filter_map(|(_, s)| s.get().borrow().public_ips())
                    .map(|addr| SkyNode::from(IpAddr::from(addr.0)))
                    .collect();

                let sky_client = SkyClient::new(server_addrs);

                let nearby_sky_id = nearby.to_sky_id(since_epoch()).into();
                let span = info_span!("client_lookup");
                let res = tokio::time::timeout(
                    Duration::from_secs(30),
                    sky_client
                        .node_lookup(
                            tp,
                            EarthNode::new(EarthId::from_array(rand::random())).into(),
                            nearby.to_sky_id(since_epoch()),
                        )
                        .instrument(span),
                )
                .await
                .unwrap();

                info!("client lookup");

                let mut all_server_addrs: Vec<_> = all_server_addrs
                    .into_iter()
                    .map(|n| (n.id().xor_distance(&nearby_sky_id), n))
                    .collect();

                let mut res: Vec<_> = res
                    .into_iter()
                    .map(|n| (n.id().xor_distance(&nearby_sky_id), n))
                    .collect();

                all_server_addrs.sort_by_key(|v| v.0.clone());
                res.sort_by_key(|v| v.0.clone());
                let comp_len = res.len().min(10);

                assert_eq!(&res[..comp_len], &all_server_addrs[..comp_len]);

                Ok(())
            }
            .with_subscriber(
                tracing_subscriber::fmt()
                    .pretty()
                    .with_writer(output_file)
                    .with_ansi(false)
                    .with_timer(tokio_uptime::tokio_uptime())
                    .with_env_filter(
                        "sky=debug,kademlia=info,shared_schema=debug,end_to_end_test=warn",
                    ),
            )
            .await
        }
    })
    .into_ref();
    let addr = client.get().borrow().connect_to_net(net);
    client.get().borrow().set_public_ips(addr);
    client
}

fn create_sky_node(
    net: MachineRef<ip::Network>,
    bootstrap_nodes: Vec<SkyNode>,
    offset: usize,
    num_inited: Rc<RefCell<usize>>,
) -> MachineRef<OsMock> {
    let node = OsMock::new(move || {
        let nodes = bootstrap_nodes.clone();
        let num_inited = num_inited.clone();
        async move {
            let ip: IpAddr = Sim::get_current_machine_ref::<OsMock>()
                .get()
                .borrow()
                .public_ips()
                .unwrap()
                .0
                .into();
            let _dir = std::fs::create_dir_all("logs").unwrap();
            let output_file = std::fs::File::create(format!("logs/server_{ip}.log")).unwrap();
            async {
                let server = SkyServer::new().await?;
                let jh = server.run();
                while *num_inited.borrow() < offset {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                tokio::time::sleep(Duration::from_millis(rand::random_range(0..5000))).await;
                server.add_nodes(nodes).await;
                server.bootstrap().await;
                tracing::info!("ready for client");
                *num_inited.borrow_mut() += 1;

                jh.await.unwrap()?;
                Ok(())
            }
            .with_subscriber(
                tracing_subscriber::fmt()
                    .pretty()
                    .with_writer(output_file)
                    .with_ansi(false)
                    .with_timer(tokio_uptime::tokio_uptime())
                    .with_env_filter(
                        "sky=info,kademlia=info,shared_schema=debug,end_to_end_test=warn",
                    ),
            )
            .await
        }
    })
    .into_ref();
    let addr = node.get().borrow().connect_to_net(net);
    node.get().borrow().set_public_ips(addr);
    node
}

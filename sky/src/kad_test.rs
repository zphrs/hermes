use std::{cell::RefCell, rc::Rc, time::Duration};

use end_to_end_test::{
    Host, OsShim,
    net::ip,
    sim::{MachineRef, Sim},
};

use kademlia::HasId;

use rand::seq::SliceRandom;
use shared_schema::{EarthNode, SkyNode, earth_node::EarthId};
use tracing::{Instrument, info, info_span, trace};

use crate::{
    client::SkyClient, get_system_time::since_epoch, quinn_transport::Transport,
    request_handler::SkyServer, tokio_uptime,
};

#[test]
fn kad() {
    let sim = Sim::new();
    sim.enter_runtime(|| {
        // Create a `fmt` subscriber that uses our custom event format, and set it
        // as the default.
        tracing_subscriber::fmt()
            .pretty()
            .with_test_writer()
            .with_timer(tokio_uptime::tokio_uptime())
            .with_env_filter(
                "sky=info,kademlia=debug,shared_schema=debug,end_to_end_test=info,rpc[{addr=\"92.168.0.140\"}]=debug",
            )
            .init();
        let net = Sim::add_machine(ip::Network::new());

        trace!("bootstrapped first node");
        let num_inited = Rc::new(RefCell::new(0));
        let mut bootstrap_servers: Vec<MachineRef<OsShim>> = vec![];
        let mut machine_addresses: Vec<SkyNode> = vec![];
        // join servers in 10 rounds of 2^i
        let mut spawned_so_far = 0;
        for i in 0..10 {
            let round_size = 2usize.pow(i);
            let mut prev_bootstrap_addresses = machine_addresses.clone();
            for j in 0..round_size {
                let (sample_of_machine_addresses, _) =
                    prev_bootstrap_addresses.partial_shuffle(&mut rand::rng(), 10);
                let new_server = create_sky_node(
                    net,
                    sample_of_machine_addresses.into(),
                    spawned_so_far,
                    num_inited.clone(),
                );
                let addr = new_server.get().borrow().public_ip().unwrap();
                machine_addresses.push(addr.into());
                bootstrap_servers.push(new_server);
            }
            spawned_so_far += round_size;
        }

        let (sample_of_servers, _) =
            bootstrap_servers.partial_shuffle(&mut rand::rng(), 10);

        let client1 = create_client(
            net,
            sample_of_servers.into(),
            machine_addresses.clone(),
            EarthId::ZERO,
            num_inited.clone(),
        );
        let client2 = create_client(
            net,
            sample_of_servers.into(),
            machine_addresses,
            EarthId::from_array([128u8; 32]),
            num_inited,
        );
        trace!("spawned {spawned_so_far} nodes");
        let idle_list = [client1, client2];

        match Sim::run_until_idle(|| idle_list.iter()) {
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
    servers: Vec<MachineRef<OsShim>>,
    all_server_addrs: Vec<SkyNode>,
    nearby: EarthId,
    num_inited: Rc<RefCell<usize>>,
) -> MachineRef<OsShim> {
    let client = OsShim::new(Host::new(move || {
        let servers = servers.clone();
        let nearby = nearby.clone();
        let all_server_addrs = all_server_addrs.clone();
        let num_inited = num_inited.clone();
        async move {
            trace!("running client");
            let tp = Transport::client().await?;
            while *num_inited.borrow() < all_server_addrs.len() {
                tokio::time::sleep(Duration::from_secs(100)).await;
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

            let server_addrs: Vec<SkyNode> = servers
                .iter()
                .enumerate()
                .filter_map(|(_, s)| s.get().borrow().public_ip())
                .map(|addr| SkyNode::from(addr))
                .collect();

            let sky_client = SkyClient::new(server_addrs);

            let nearby_sky_id = nearby.to_sky_id(since_epoch()).into();
            let span = info_span!("client_lookup");
            let res = tokio::time::timeout(
                Duration::from_secs(30),
                sky_client
                    .node_lookup(
                        tp,
                        EarthNode::new(EarthId::ZERO).into(),
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
    }));
    let addr = client.get().borrow().connect_to_net(net);
    client.get().borrow().set_public_ip(addr);
    client
}

fn create_sky_node(
    net: MachineRef<ip::Network>,
    bootstrap_nodes: Vec<SkyNode>,
    offset: usize,
    num_inited: Rc<RefCell<usize>>,
) -> MachineRef<OsShim> {
    let node = OsShim::new(Host::new(move || {
        let nodes = bootstrap_nodes.clone();
        let num_inited = num_inited.clone();
        async move {
            let span = info_span!(
                "sky node",
                addr = %Sim::get_current_machine::<OsShim>()
                    .borrow()
                    .public_ip()
                    .unwrap()
            );
            async {
                let server = SkyServer::new().await?;
                let jh = server.run();
                while *num_inited.borrow() < offset {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
                tokio::time::sleep(Duration::from_secs(rand::random_range(0..10))).await;
                server.add_nodes(nodes).await;
                server.bootstrap().await;
                tracing::info!("ready for client");
                *num_inited.borrow_mut() += 1;

                jh.await.unwrap()?;
                Ok(())
            }
            .instrument(span)
            .await
        }
    }));
    let addr = node.get().borrow().connect_to_net(net);
    node.get().borrow().set_public_ip(addr);
    node
}

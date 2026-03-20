use std::time::Duration;

use end_to_end_test::{
    Host, OsShim,
    net::ip,
    sim::{MachineRef, Sim},
};
use rpc::{Caller, Transport as _};
use shared_schema::{EarthNode, SkyNode, earth_node::EarthId};
use tracing_subscriber::fmt::time::tokio_uptime;

use crate::{
    client::KadClient,
    quinn_transport::Transport,
    request_handler::{FindNodesMethod, RootHandler, RootRequest, SkyServer},
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
            .with_timer(tokio_uptime())
            .with_env_filter("sky::client=debug,shared_schema=debug,end_to_end_test=info,rpc=warn")
            .init();
        let net = Sim::add_machine(ip::Network::new());
        let bootstrap = new_node(net, vec![]);

        let bootstrap_addr = bootstrap.get().borrow().public_ip().unwrap();

        let mut servers: Vec<_> = (0..10)
            .map(|_| new_node(net, vec![bootstrap_addr.into()]))
            .collect();

        servers.push(bootstrap);

        let client = OsShim::new(Host::new(move || {
            let servers = servers.clone();
            async move {
                let tp = Transport::client().await?;
                tokio::time::sleep(Duration::from_secs(1)).await;
                tokio::time::advance(Duration::from_mins(100)).await;
                // choose 1/10 of the servers
                let server_addrs: Vec<SkyNode> = servers
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i % 10 == 0)
                    .filter_map(|(_, s)| s.get().borrow().public_ip())
                    .map(|addr| SkyNode::from(addr))
                    .collect();
                let kad_client = KadClient::new(EarthNode::new(EarthId::ZERO), server_addrs, tp);
                let res = kad_client.node_lookup(EarthId::ZERO).await;
                expect_test::expect![[r#"
                    [
                        SkyNode {
                            address: 192.168.0.1,
                            id: Id(
                                65B6B02D8CB9CA36
                                2E22849EABDF63E1
                                6BF5590C1562BAD2
                                BF87106BF35002CD,
                            ),
                        },
                        SkyNode {
                            address: 192.168.0.2,
                            id: Id(
                                89F837189AD27E2A
                                A809A00AD5C22757
                                5F70C91C384DBB1E
                                43BC297F926EA707,
                            ),
                        },
                    ]
                "#]].assert_debug_eq(&res);
                Ok(())
            }
        }));
        let addr = client.get().borrow().connect_to_net(net);
        client.get().borrow().set_public_ip(addr);
        let idle_list = [client];
        Sim::run_until_idle(|| idle_list.iter()).unwrap();
    })
}

fn new_node(net: MachineRef<ip::Network>, bootstrap_nodes: Vec<SkyNode>) -> MachineRef<OsShim> {
    let node = OsShim::new(Host::new(move || {
        let nodes = bootstrap_nodes.clone();
        async move {
            let server = SkyServer::new().await?;
            server.add_nodes(nodes).await;
            server.run().await?;
            Ok(())
        }
    }));
    let addr = node.get().borrow().connect_to_net(net);
    node.get().borrow().set_public_ip(addr);
    node
}

use kademlia::HasId as _;
use std::time::{Duration, SystemTime};

use end_to_end_test::{
    Host, OsShim,
    net::ip,
    sim::{MachineRef, Sim},
};
use rpc::{Caller, Transport as _};
use shared_schema::{EarthNode, SkyNode, earth_node::EarthId};

use crate::{
    client::{KadClient, get_system_time},
    quinn_transport::Transport,
    request_handler::{FindNodesMethod, RootHandler, RootRequest, SkyServer},
    tokio_uptime,
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
                "sky=debug,kademlia=trace,shared_schema=debug,end_to_end_test=warn,rpc=warn",
            )
            .init();
        let net = Sim::add_machine(ip::Network::new());
        let bootstrap = new_node(net, vec![]);

        let bootstrap_addr = bootstrap.get().borrow().public_ip().unwrap();

        let mut servers: Vec<_> = (0..100)
            .map(|_| new_node(net, vec![bootstrap_addr.into()]))
            .collect();

        servers.push(bootstrap);

        let client = OsShim::new(Host::new(move || {
            let servers = servers.clone();
            async move {
                let tp = Transport::client().await?;
                tokio::time::sleep(Duration::from_secs(200)).await;
                tokio::time::advance(Duration::from_mins(10000)).await;
                // choose 1/10 of the servers
                let server_addrs: Vec<SkyNode> = servers
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| i % 10 == 0)
                    .filter_map(|(_, s)| s.get().borrow().public_ip())
                    .map(|addr| SkyNode::from(addr))
                    .collect();
                let kad_client = KadClient::new(EarthNode::new(EarthId::ZERO), server_addrs, tp);
                let mut res = kad_client.node_lookup(EarthId::ZERO).await;
                let earth_server_id: kademlia::Id<32> = EarthId::ZERO
                    .to_sky_id(
                        get_system_time()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap(),
                    )
                    .into();
                res.sort_by_key(|n| n.id() ^ &earth_server_id);
                expect_test::expect![[r#"
                    [
                        SkyNode {
                            address: 192.168.0.42,
                            id: Id(
                                D4F919569577E03F
                                E7C99582D8A999CF
                                A11F32482DFB2359
                                9B43DAC116E5C8AC,
                            ),
                        },
                        SkyNode {
                            address: 192.168.0.62,
                            id: Id(
                                DBF83026CED2044D
                                93FFB63BCCC5D41A
                                06BF8E28E463AECC
                                2683121E43FF786E,
                            ),
                        },
                        SkyNode {
                            address: 192.168.0.92,
                            id: Id(
                                DBBDBD8F5E7F8DFA
                                3DE5C5C391105B47
                                6264CC97A4E26059
                                E575190106F1360A,
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
                        SkyNode {
                            address: 192.168.0.22,
                            id: Id(
                                8C239005AACD4269
                                9C45FCD33B873A7A
                                4F334F7065120A02
                                55DF28C01075FA79,
                            ),
                        },
                        SkyNode {
                            address: 192.168.0.72,
                            id: Id(
                                7E72D6270DD0A27E
                                388F2C8D569DBC8B
                                A02CAFDCD3F32306
                                248A13AF228B1C3A,
                            ),
                        },
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
                            address: 192.168.0.32,
                            id: Id(
                                6CFD1B448ACC1F12
                                06E801C03C22B0D0
                                E4D9B433EA8E1644
                                1F9BFBC0B83F4D1F,
                            ),
                        },
                        SkyNode {
                            address: 192.168.0.82,
                            id: Id(
                                54DF240288936334
                                14F3D4C1B8E45F64
                                1D03019DC5E645E2
                                885E18A9CCE0C2CA,
                            ),
                        },
                        SkyNode {
                            address: 192.168.0.52,
                            id: Id(
                                3D92CACEAE3ADA33
                                052F39387234921E
                                89EE2E041E3CD9EA
                                DE17FC9C6D95250D,
                            ),
                        },
                        SkyNode {
                            address: 192.168.0.12,
                            id: Id(
                                24717EC09AD8A44A
                                F954610BF0D98092
                                464EFBBB1E59AF97
                                B641EE108E1A7DD5,
                            ),
                        },
                    ]
                "#]]
                .assert_debug_eq(&res);
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

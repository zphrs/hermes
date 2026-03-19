use end_to_end_test::{
    Host, OsShim,
    net::ip,
    sim::{MachineRef, Sim},
};
use rpc::{Caller, Transport as _};
use shared_schema::SkyNode;

use crate::{
    quinn_transport::Transport,
    request_handler::{FindNodesMethod, RootHandler, RootRequest, SkyServer},
};

#[test]
fn kad() {
    let sim = Sim::new();
    sim.enter_runtime(|| {
        let net = Sim::add_machine(ip::Network::new());
        let bootstrap = new_node(net, []);

        let bootstrap_addr = bootstrap.get().borrow().public_ip().unwrap();

        let mut servers: Vec<_> = (0..100)
            .map(|_| new_node(net, [bootstrap_addr.into()]))
            .collect();

        servers.push(bootstrap);

        let client = OsShim::new(Host::new(move || {
            let servers = servers.clone();
            async move {
                let server_addrs: Vec<SkyNode> = servers
                    .iter()
                    .map(|v| v.get().borrow().public_ip().unwrap().into())
                    .collect();

                let tp = Transport::client().await?;

                let conn = Transport::connect(&tp, server_addrs.first().unwrap()).await?;

                let client = FindNodesMethod::new(&tp, );



                let (handler, tp) = SkyServer::new().await?.into_parts();



                handler

                Ok(())
            }
        }));
    })
}

fn new_node(
    net: MachineRef<ip::Network>,
    bootstrap_nodes: impl IntoIterator<Item = SkyNode> + Clone + 'static,
) -> MachineRef<OsShim> {
    // create one bootstrap server with ip 1.2.3.4
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

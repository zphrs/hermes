//! based on the [api of simgrid](https://simgrid.org/doc/latest/app_s4u.html)
pub(crate) mod config;
mod error;
mod host;
pub mod net;
pub mod sim;

pub use net::udp::Packet;

/// hosts can belong to multiple networks

#[cfg(test)]
mod tests {
    use crate::{
        host::{Host, os_shim::OsShim},
        net::{
            ip,
            udp::{self},
        },
        sim::{
            ACTIVE_MACHINE_ID, Sim,
            machine::{Machine as _, MachineId},
        },
    };
    use bytes::BytesMut;
    use std::{cell::RefCell, collections::HashMap, mem::MaybeUninit, rc::Rc, time::Duration};
    use tracing::trace;

    #[test]
    fn quinn() {}

    #[test]
    fn os() {
        let mut sim = Sim::new();
        let host = sim.add_machine(OsShim::new(Host::new(10, || async { Ok(()) })));
    }

    #[test]
    fn send_message() {
        let sim = Rc::new(RefCell::new(Sim::new()));
        let ipv4_net = Rc::new(RefCell::new(ip::Network::new()));
        let host_store: Rc<RefCell<HashMap<MachineId, Rc<RefCell<Host>>>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let host_store2 = host_store.clone();
        let host = sim.borrow_mut().add_machine(Host::new(10, move || {
            let host_store2 = host_store2.clone();
            async move {
                let host_id = ACTIVE_MACHINE_ID.with(|host| *host);
                let borrowed_store = host_store2.borrow();

                let host = borrowed_store.get(&host_id).unwrap();
                let mut res = host.borrow().read().await.expect("valid res");
                let packet = udp::Packet::try_from_bytes(&mut res).unwrap();

                assert!(packet.check_checksum());

                assert_eq!(packet.body()[..], b"Hello world!"[..]);

                Ok(())
            }
        }));
        host_store
            .borrow_mut()
            .insert(*host.borrow().id(), host.clone());
        let prefix = "127.0.0.0/24".parse().unwrap();
        ipv4_net
            .borrow_mut()
            .add_machine(&*host.borrow(), Some(prefix));

        let ipv4_net2 = ipv4_net.clone();
        let host2 = sim.borrow_mut().add_machine(Host::new(10, move || {
            let ipv4_net = ipv4_net2.clone();
            async move {
                let mut packet = BytesMut::new();
                udp::Packet::new(
                    "127.0.0.1:8000".parse()?,
                    "127.0.0.1:8080".parse()?,
                    b"Hello world!",
                    None,
                )
                .write_into_buf(&mut packet);
                ipv4_net.borrow_mut().try_send_packet(packet.freeze())?;
                Ok(())
            }
        }));
        host2.borrow().start();
        sim.borrow().enter_runtime(|| {
            host2.borrow().tick(Duration::from_millis(1)).unwrap();
        });
        sim.borrow().enter_runtime(|| {
            ipv4_net
                .borrow_mut()
                .network_mut()
                .tick(Duration::from_millis(500))
                .unwrap();
        });
        host.borrow().start();
        sim.borrow()
            .on_machine(&*host.borrow(), || {
                host.borrow().tick(Duration::from_millis(1))
            })
            .unwrap();
        let host_is_idle = host.borrow().is_idle();
        assert!(host_is_idle);

        let host2_is_idle = host2.borrow().is_idle();
        assert!(host2_is_idle);
        host2.borrow().stop();

        // sim.root_network().try_send_to_host(host1.id())
    }
}

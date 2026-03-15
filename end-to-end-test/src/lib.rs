#![allow(dead_code)] // TODO: remove this line to check for dead code
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
        sim::{Sim, machine::Machine as _},
    };
    use bytes::BytesMut;
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        time::Duration,
    };
    use tracing::trace;
    use tracing_test::traced_test;

    #[test]
    fn quinn() {}

    #[test]
    #[traced_test]
    fn os() {
        let sim = Sim::new();
        sim.enter_runtime(|| {
            let net = Sim::add_machine(ip::Network::new());
            const SERVER_ADDR: SocketAddr =
                SocketAddr::new(IpAddr::V4(Ipv4Addr::from_octets([192, 168, 0, 1])), 3000);
            let server = OsShim::new(Host::new(10, move || async move {
                use crate::host::net::udp;

                let socket = udp::UdpSocket::bind(SERVER_ADDR).await?;
                println!("running server");
                let mut buf = [0u8; b"Hello world!".len()];
                let (_len, _addr) = socket.recv_from(&mut buf).await?;
                assert_eq!(&buf, b"Hello world!");
                Ok(())
            }));

            server.get().borrow().connect_to_net(net);

            let client = OsShim::new(Host::new(10, || async {
                use crate::host::net::udp;
                let socket =
                    udp::UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).await?;
                trace!("client socket inited");
                socket.connect(SERVER_ADDR).await?;
                trace!("client connected to server");
                socket.send(b"Hello world!").await?;
                trace!("client sent message, DONE");
                Ok(())
            }));
            client.get().borrow().connect_to_net(net);

            Sim::tick_machine(server).unwrap();
            Sim::tick_machine(client).unwrap();
            assert!(client.is_idle());
            for _ in 0..50 {
                Sim::tick_machine(net).unwrap();
            }
            assert!(Sim::tick_machine(net).unwrap());
            Sim::tick_machine(server).unwrap();

            assert!(net.is_idle());

            assert!(server.is_idle());
        })
    }

    #[test]
    #[traced_test]
    fn send_message() {
        let sim = Sim::new();
        sim.enter_runtime(|| {
            let ipv4_net = Sim::add_machine(ip::Network::new());
            let server = Sim::add_machine(Host::new(10, move || {
                #[allow(clippy::await_holding_refcell_ref)]
                async move {
                    println!("ENTERED HOST");
                    // add self to ipv4_net

                    let host = Sim::get_current_machine::<Host>();
                    ipv4_net.get().borrow_mut().add_machine_with_range(
                        &*host.borrow(),
                        IpAddr::from(Ipv4Addr::LOCALHOST).into(),
                    );

                    println!("HOST AWAITING MSG");

                    let res = host.borrow().inner().read().await.expect("valid res");
                    println!("HOST GOT MSG");

                    let packet = udp::Packet::try_from_bytes(res).unwrap();

                    assert!(packet.check_checksum());

                    assert_eq!(packet.body()[..], b"Hello world!"[..]);
                    println!("HOST FINISHED");

                    Ok(())
                }
            }));

            let client = Sim::add_machine(Host::new(10, move || async move {
                println!("ENTERED CLIENT");
                let mut packet = BytesMut::new();

                udp::Packet::new(
                    "127.0.0.1:8000".parse()?,
                    "127.0.0.1:8080".parse()?,
                    b"Hello world!",
                    None,
                )
                .write_into_buf(&mut packet);
                ipv4_net
                    .get()
                    .borrow_mut()
                    .try_send_packet(packet.freeze())
                    .unwrap();
                Ok(())
            }));

            server.get().borrow().start();

            Sim::on_machine(server, || {
                assert!(
                    !Sim::get_current_machine::<Host>()
                        .borrow()
                        .tick(Duration::from_millis(1000))
                        .unwrap()
                );
            });

            client.get().borrow().start();

            client
                .get()
                .borrow()
                .tick(Duration::from_millis(1))
                .unwrap();

            ipv4_net
                .get()
                .borrow_mut()
                .tick(Duration::from_millis(500)) // max message latency
                .unwrap();

            // sent message

            server
                .get()
                .borrow()
                .tick(Duration::from_millis(1))
                .unwrap();

            let host_is_idle = sim.enter_runtime(|| server.get().borrow().is_idle());
            assert!(host_is_idle);

            let client_is_idle = sim.enter_runtime(|| client.get().borrow().is_idle());
            assert!(client_is_idle);
            client.get().borrow().stop();
        })

        // sim.root_network().try_send_to_host(host1.id())
    }
}

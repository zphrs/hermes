//! Combines the shape of [simgrid's
//! API](https://simgrid.org/doc/latest/app_s4u.html) and
//! [turmoil](https://crates.io/crates/turmoil)'s use of Tokio to simulate
//! machines in order to allow Turmoil-like simulations but with arbitrary
//! network interfaces.
//!
//! Out of the box there is the [OsShim] which can be used to shim out tokio UDP
//! sockets by substituting out Tokio's [UdpSocket](tokio::net::UdpSocket) with
//! [UdpSocket].
//!
//! Ping client example:
//!
//! ```rust
//! # use std::time::Duration;
//! # use dens::sim::{self, Sim, Config, config::MessageLoss};
//! # use dens::net::ip;
//! # use dens::host::{Host, os_shim::OsShim};
//! # use std::net::{IpAddr, Ipv6Addr};
//! let sim = Sim::new_with_config(sim::Config {
//!     message_loss: MessageLoss { fail_rate: 0.0 },
//!     ..Default::default()
//! });
//! sim.enter_runtime(|| {
//!     let net = Sim::add_machine(ip::Network::new());
//!     let server = OsShim::new(Host::new(move || async move {
//!         use dens::host::net::udp;
//!         // bind
//!         let socket = udp::UdpSocket::bind((Ipv6Addr::from(0u128), 3000)).await?;
//!         // recv ping
//!         let mut buf = [0u8; "ping".len()]; // max of "Hello world!" and "yay" lengths, but we need to handle variable sizes
//!         let (_len, addr) = socket.recv_from(&mut buf).await?;
//!         assert_eq!(&buf, b"ping");
//!         // send pong
//!         socket.send_to(b"pong", addr).await?;
//!         Ok(())
//!     }));
//!     let (_, server_addr) = server.get().borrow().connect_to_net(net);
//!
//!     let client = OsShim::new(Host::new(move || async move {
//!         use dens::host::net::udp;
//!         // connect
//!         let socket = udp::UdpSocket::bind((Ipv6Addr::from(0u128), 3000)).await?;
//!         socket.connect((server_addr, 3000)).await?;
//!         // send ping
//!         let _len = socket.send(b"ping").await?;
//!         // recv pong
//!         let mut buf = [0u8; "pong".len()];
//!         let _len = socket.recv(&mut buf).await?;
//!         assert_eq!(&buf, b"pong");
//!         Ok(())
//!     }));
//!     client.get().borrow().connect_to_net(net);
//!
//!     let arr = [server, client];
//!     Sim::run_until_idle(|| arr.iter()).unwrap();
//! })
//!
//! ```
//!
//! While you likely want to use the OsShim most of the time, it is also
//! possible to define a machine, such as a router, which has different logic
//! for manipulating packets than the logic which an OS has by default. For an
//! example of using the primitives exposed in this library (namely
//! [`ip::Network`], [`BasicMachine`]) to create a more complex machine, see
//! [`Nat`].
//!
//! Specifically the [OsShim] assumes that the machine has a finite list of IP
//! addresses (one for each network) while the more abstract [BasicMachine]
//! allows arbitrary manipulation of IP packets.
//!
mod deterministic_rand;
mod error;
pub mod host;
pub use host::net::udp::UdpSocket;
pub mod net;
pub mod sim;
pub use host::Host;
pub use net::Network;
pub use net::ip;
pub use sim::Sim;
pub use sim::machine::BasicMachine;

pub use host::os_shim::OsShim;

pub use net::udp::Packet;
pub use sim::machine::Machine;

/// hosts can belong to multiple networks
#[cfg(test)]
mod tests {
    use crate::{
        host::{Host, os_shim::OsShim},
        net::{
            ip,
            udp::{self},
        },
        sim::{self, Sim, config::MessageLoss, machine::Machine},
    };
    use bytes::BytesMut;
    use std::{
        net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
        time::Duration,
    };
    use tracing::trace;

    #[test]
    fn quinn() {}

    #[test]
    fn peer_addr() {
        use crate::UdpSocket;
        use crate::sim::Sim;
        use crate::{Host, OsShim};
        use std::net::SocketAddr;

        let sim = Sim::new();
        sim.enter_runtime(|| {
            let shim = OsShim::new(Host::new(|| async {
                let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8080));
                let peer = "127.0.0.1:11100".parse::<SocketAddr>().unwrap();
                let sock = UdpSocket::bind(addr).await?;
                sock.connect(peer).await?;
                assert_eq!(peer, sock.peer_addr()?);
                Ok(())
            }));
            let arr = [shim];
            Sim::run_until_idle(|| arr.iter()).unwrap();
        });
    }

    #[test]
    fn simple_os() {
        let sim = Sim::new_with_config(sim::Config {
            message_loss: MessageLoss { fail_rate: 0.0 },
            ..Default::default()
        });
        sim.enter_runtime(|| {
            let net = Sim::add_machine(ip::Network::new());
            let server = OsShim::new(Host::new(move || async move {
                use crate::host::net::udp;
                // bind
                let socket = udp::UdpSocket::bind((Ipv6Addr::from(0u128), 3000)).await?;
                // recv ping
                let mut buf = [0u8; "ping".len()]; // max of "Hello world!" and "yay" lengths, but we need to handle variable sizes
                let (_len, addr) = socket.recv_from(&mut buf).await?;
                assert_eq!(&buf, b"ping");
                // send pong
                socket.send_to(b"pong", addr).await?;
                Ok(())
            }));
            let (_, server_addr) = server.get().borrow().connect_to_net(net);

            let client = OsShim::new(Host::new(move || async move {
                use crate::host::net::udp;
                // connect
                let socket = udp::UdpSocket::bind((Ipv6Addr::from(0u128), 3000)).await?;
                socket.connect((server_addr, 3000)).await?;
                // send ping
                let _len = socket.send(b"ping").await?;
                // recv pong
                let mut buf = [0u8; "pong".len()];
                let _len = socket.recv(&mut buf).await?;
                assert_eq!(&buf, b"pong");
                Ok(())
            }));
            client.get().borrow().connect_to_net(net);

            let arr = [server, client];
            Sim::run_until_idle(|| arr.iter()).unwrap();
        })
    }

    #[test_log::test]
    fn os() {
        let sim = Sim::new_with_config(sim::Config {
            message_loss: MessageLoss { fail_rate: 0.0 },
            ..Default::default()
        });
        sim.enter_runtime(|| {
            let net = Sim::add_machine(ip::Network::new());
            let server = OsShim::new(Host::new(move || async move {
                use crate::host::net::udp;

                let socket = udp::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 3000)).await?;
                let mut messages = std::collections::HashSet::new();

                let mut buf = [0u8; 13]; // max of "Hello world!" and "yay" lengths, but we need to handle variable sizes
                let (len, _addr) = socket.recv_from(&mut buf).await?;
                messages.insert(String::from_utf8_lossy(&buf[..len]).to_string());

                let mut buf = [0u8; 13];
                let (len, _addr) = socket.recv_from(&mut buf).await?;
                messages.insert(String::from_utf8_lossy(&buf[..len]).to_string());

                assert!(messages.contains("Message 1"));
                assert!(messages.contains("Message 2"));
                Ok(())
            }));

            let (server_addr, _) = server.get().borrow().connect_to_net(net);

            let client = OsShim::new(Host::new(move || async move {
                use crate::host::net::udp;
                let socket =
                    udp::UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).await?;
                trace!("client socket inited");
                socket.connect((server_addr, 3000)).await?;
                trace!("client connected to server");
                socket.send(b"Message 1").await?;
                trace!("client sent message 2");
                socket.send(b"Message 2").await?;
                Ok(())
            }));
            let addr = client.get().borrow().connect_to_net(net);
            client.get().borrow().set_public_ips(addr);
            let arr = [client, server];
            Sim::run_until_idle(|| arr.iter()).unwrap();

            assert!(net.get().borrow().is_idle());
            assert!(client.get().borrow().is_idle());
            assert!(server.get().borrow().is_idle());
        })
    }

    #[test]
    #[test_log::test]
    fn send_message() {
        let sim = Sim::new();
        sim.enter_runtime(|| {
            let ipv4_net = Sim::add_machine(ip::Network::new());
            let server = Sim::add_machine(Host::new(move || {
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

            let client = Sim::add_machine(Host::new(move || async move {
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
            Sim::tick_machine(server).unwrap();
            Sim::tick_machine(client).unwrap();

            ipv4_net
                .get()
                .borrow_mut()
                .basic_machine()
                .tick(Duration::from_millis(500)) // max message latency
                .unwrap();

            // sent message

            Sim::tick_machine(server).unwrap();

            let host_is_idle = sim.enter_runtime(|| server.get().borrow().is_idle());
            assert!(host_is_idle);

            let client_is_idle = sim.enter_runtime(|| client.get().borrow().is_idle());
            assert!(client_is_idle);
            client.get().borrow().stop();
        })

        // sim.root_network().try_send_to_host(host1.id())
    }
}

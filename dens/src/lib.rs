//! Combines the shape of [simgrid's
//! API](https://simgrid.org/doc/latest/app_s4u.html) and
//! [turmoil](https://crates.io/crates/turmoil)'s use of Tokio to simulate
//! machines in order to allow Turmoil-like simulations but with arbitrary
//! network interfaces.
//!
//! Out of the box there is [`OsMock`] which can be used to mock out tokio UDP
//! sockets by substituting out use of Tokio's [`UdpSocket`](tokio::net::UdpSocket) with
//! [`os_mock::net::UdpSocket`] depending on whether the binary was compiled for tests:
//!
//! ```rust
//! #[cfg(not(test))]
//! use tokio::net::UdpSocket;
//! #[cfg(test)]
//! use dens::os_mock::net::UdpSocket;
//! ```
//!
//! # Examples
//!
//! ## Ping client
//!
//! ```rust
//! use std::net::{IpAddr, Ipv6Addr};
//! use dens::{net::ip, OsMock, sim::{self, MachineIntoRef}, Sim};
//! let sim = Sim::new_with_config(sim::Config::synchronous_network());
//! sim.enter_runtime(|| {
//!     let net = ip::Network::new_private_class_c().into_ref();
//!     let server = OsMock::new(move || async move {
//!         use dens::os_mock::net::UdpSocket;
//!         // bind
//!         let socket = UdpSocket::bind((Ipv6Addr::from(0u128), 3000)).await?;
//!         // recv ping
//!         let mut buf = [0u8; "ping".len()]; // max of "Hello world!" and "yay" lengths, but we need to handle variable sizes
//!         let (_len, addr) = socket.recv_from(&mut buf).await?;
//!         assert_eq!(&buf, b"ping");
//!         // send pong
//!         socket.send_to(b"pong", addr).await?;
//!         Ok(())
//!     })
//!     .into_ref();
//!     let (_, server_addr) = server.get().borrow().connect_to_net(net);
//!
//!     let client = OsMock::new(move || async move {
//!         use dens::os_mock::net::UdpSocket;
//!         // connect
//!         let socket = UdpSocket::bind((Ipv6Addr::from(0u128), 3000)).await?;
//!         socket.connect((server_addr, 3000)).await?;
//!         // send ping
//!         let _len = socket.send(b"ping").await?;
//!         // recv pong
//!         let mut buf = [0u8; "pong".len()];
//!         let _len = socket.recv(&mut buf).await?;
//!         assert_eq!(&buf, b"pong");
//!         Ok(())
//!     })
//!     .into_ref();
//!     client.get().borrow().connect_to_net(net);
//!
//!     let arr = [server, client];
//!     Sim::run_until_idle(|| arr.iter()).unwrap();
//! });
//!
//! ```
//!
//! # Custom Machines
//!
//! While you likely want to use [`OsMock`] most of the time, it is also
//! possible to define a machine, such as a router, which has different logic
//! for manipulating packets than the logic which an OS has by default. For an
//! example of using the primitives exposed in this library (namely
//! [`net::ip::Network`] and [`BasicMachine`]) to create a more complex machine,
//! look at the source code for [`Nat`](net::ip::nat::Nat).
//!
//! Specifically [`OsMock`] assumes that the machine has a finite list of IP
//! addresses (one for each network) while the more abstract [`BasicMachine`]
//! allows arbitrary manipulation of IP packets.

#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::doc_markdown)]
mod deterministic_rand;
mod error;
pub mod host;
pub mod net;
pub mod sim;
pub use host::Host;
pub use net::Network;
pub use net::ip::Network as IpNetwork;
pub use sim::{MachineIntoRef, Sim, machine::BasicMachine};

pub use os_mock::OsMock;
pub mod os_mock;

pub use sim::machine::Machine;

/// hosts can belong to multiple networks
#[cfg(test)]
mod tests {
    use crate::{
        host::Host,
        net::{
            ip,
            udp::{self},
        },
        os_mock::{OsMock, net::UdpSocket},
        sim::{self, MachineIntoRef as _, Sim, config::MessageLoss, machine::Machine},
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
        use crate::{OsMock, os_mock::net::UdpSocket, sim::Sim};
        use std::net::SocketAddr;

        let sim = Sim::new();
        sim.enter_runtime(|| {
            let mock = OsMock::new(|| async {
                let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8080));
                let peer = "127.0.0.1:11100".parse::<SocketAddr>().unwrap();
                let sock = UdpSocket::bind(addr).await?;
                sock.connect(peer).await?;
                assert_eq!(peer, sock.peer_addr()?);
                Ok(())
            })
            .into_ref();
            let arr = [mock];
            Sim::run_until_idle(|| arr.iter()).unwrap();
        });
    }

    #[test_log::test]
    fn simple_os() {
        let sim = Sim::new_with_config(sim::Config {
            message_loss: MessageLoss { fail_rate: 0.0 },
            ..Default::default()
        });
        sim.enter_runtime(|| {
            let net = ip::Network::new_private_class_c().into_ref();
            let server = OsMock::new(move || async move {
                use crate::os_mock::net::UdpSocket;
                // bind
                let socket = UdpSocket::bind((Ipv6Addr::from(0u128), 3000)).await?;
                // recv ping
                let mut buf = [0u8; "ping".len()]; // max of "Hello world!" and "yay" lengths, but we need to handle variable sizes
                let (_len, addr) = socket.recv_from(&mut buf).await?;
                assert_eq!(&buf, b"ping");
                // send pong
                socket.send_to(b"pong", addr).await?;
                Ok(())
            })
            .into_ref();
            let (_, server_addr) = server.get().borrow().connect_to_net(net);

            let client = OsMock::new(move || async move {
                use crate::os_mock::net::UdpSocket;
                // connect
                let socket = UdpSocket::bind((Ipv6Addr::from(0u128), 3000)).await?;
                socket.connect((server_addr, 3000)).await?;
                // send ping
                let _len = socket.send(b"ping").await?;
                // recv pong
                let mut buf = [0u8; "pong".len()];
                let _len = socket.recv(&mut buf).await?;
                assert_eq!(&buf, b"pong");
                Ok(())
            })
            .into_ref();
            client.get().borrow().connect_to_net(net);

            let arr = [server, client];
            Sim::run_until_idle(|| arr.iter()).unwrap();
        });
    }

    #[test_log::test]
    fn os() {
        let sim = Sim::new_with_config(sim::Config::synchronous_network());
        sim.enter_runtime(|| {
            let net = ip::Network::new_private_class_c().into_ref();
            let server = OsMock::new(move || async move {
                let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 3000)).await?;
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
            })
            .into_ref();

            let (server_addr, _) = server.get().borrow().connect_to_net(net);

            let client = OsMock::new(move || async move {
                let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).await?;
                trace!("client socket inited");
                socket.connect((server_addr, 3000)).await?;
                trace!("client connected to server");
                socket.send(b"Message 1").await?;
                trace!("client sent message 2");
                socket.send(b"Message 2").await?;
                trace!("made it here");
                Ok(())
            });
            client.connect_to_net(net);
            let client = client.into_ref();
            let arr = [client, server];
            Sim::run_until_idle(|| arr.iter()).unwrap();

            assert!(net.get().borrow().is_idle());
            assert!(client.get().borrow().is_idle());
            assert!(server.get().borrow().is_idle());
        });
    }

    #[test_log::test]
    fn send_message() {
        let sim = Sim::new();
        sim.enter_runtime(|| {
            let ipv4_net = ip::Network::new_private_class_c().into_ref();
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

                    let res = host.borrow().inner().try_read().expect("valid res");
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
        });
    }
}

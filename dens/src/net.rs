use tracing::Instrument as _;
mod checksum;
pub mod error;
pub mod ip;
pub use ip::Ipv4Prefix;
pub mod nat;
pub mod udp;

use crate::sim::{
    RNG,
    config::CONFIG,
    machine::{BasicMachine, HasNic, Machine, MachineId, MachineNic},
};
use bytes::Bytes;
use rand::Rng;
use tracing::trace;

use std::rc::Rc;
use std::time::Duration;
use std::{collections::HashMap, io::ErrorKind};

pub struct Network {
    machines: HashMap<MachineId, (MachineNic, Duration)>,
    inner_machine: Rc<BasicMachine>,
}

impl Network {
    pub fn new() -> Self {
        let machine = CONFIG.with(|cfg| BasicMachine::new(cfg.ip_hop_capacity()));
        let out = Self {
            inner_machine: machine.into(),
            machines: Default::default(),
        };
        out
    }
}

impl Default for Network {
    fn default() -> Self {
        Self::new()
    }
}

impl Network {
    pub fn add_machine(&mut self, host: &impl HasNic) {
        let latency = CONFIG.with(|cfg| {
            let latency = cfg.latency();
            RNG.with(|rng| latency.sample(rng.borrow_mut()))
        });
        self.machines.insert(host.id(), (host.nic(), latency));
    }

    pub fn remove_machine(&mut self, host: &impl HasNic) {
        self.machines.remove(&host.id());
    }

    pub fn try_send_to_host(&self, id: &MachineId, posting: Bytes) -> std::io::Result<()> {
        let dropped = CONFIG.with(|cfg| {
            let dropped =
                RNG.with(|rng| rng.borrow_mut().random_bool(cfg.message_loss_fail_rate()));
            dropped
        });
        let (nic, latency) = self
            .machines
            .get(id)
            .ok_or(ErrorKind::HostUnreachable)?
            .clone();
        let jitter = RNG.with(|rng| {
            let stdev = latency.as_millis() as f64 / 5.0;
            let dist = rand_distr::Normal::new(0.0, stdev).unwrap();
            let jitter_ms = rng.borrow_mut().sample(dist) as i64;
            if jitter_ms < 0 {
                latency.saturating_sub(Duration::from_millis((-jitter_ms) as u64))
            } else {
                latency + Duration::from_millis(jitter_ms as u64)
            }
        });
        let span = tracing::debug_span!("send_to_host", to=?id);
        self.inner_machine.spawn_local(
            async move {
                if dropped {
                    trace!("dropped message due to chance");
                    return Ok(());
                }
                trace!("waiting to send for {:?}", jitter);
                tokio::time::sleep(jitter).await;
                let res = nic.try_post(posting).await;
                if let Err(e) = res {
                    tracing::warn!("failed to deliver message to nic: {}", e);
                }
                Ok(())
            }
            .instrument(span),
        );

        Ok(())
    }
}

impl Machine for Network {
    fn basic_machine(&self) -> Rc<BasicMachine> {
        self.inner_machine.clone()
    }

    fn is_idle(&self) -> bool {
        self.inner_machine.is_idle()
    }
}

#[cfg(test)]
mod tests {
    pub use crate::net::Ipv4Prefix;
    use std::{
        io::ErrorKind,
        net::{Ipv4Addr, SocketAddr},
        time::Duration,
    };

    use bytes::BytesMut;
    use tokio::time::sleep;
    use tracing::{info, trace};

    use crate::{
        Host, Machine, OsShim, Sim,
        host::net::udp,
        net::{
            ip::Network,
            nat::{self},
        },
        sim::Config,
    };

    #[test_log::test]
    fn nat_basic() {
        let s = Sim::new_with_config(Config {
            udp_capacity: 1,
            ip_hop_capacity: 1,
            nic_capacity: 1,
            ..Config::new_with_sync_network()
        });
        s.enter_runtime(|| {
            let net = Sim::add_machine(Network::new_with_ipv4_prefix(
                Ipv4Prefix::new([192, 0, 2, 0].into(), 24).unwrap(),
            ));
            let server = OsShim::new(Host::new(move || async move {
                let socket = udp::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 3000)).await?;
                loop {
                    let mut buf = BytesMut::new();
                    let Ok((_, addr)) = socket.recv_buf_from(&mut buf).await else {
                        continue;
                    };
                    trace!("received {buf:?} from {addr}");
                    socket.send_to(&buf, addr).await.unwrap();
                }
            }));
            // need to assign public ip address manually to avoid colliding with the nat ip addresses
            // in other words, we need an ip that is not an internal network ip address
            let (server_addr, _) = server.get().borrow().connect_to_net(net);
            info!("server address: {server_addr}");
            let nat = Sim::add_machine(nat::Nat::<nat::hard::Symmetric>::new(net));
            let client = OsShim::new(Host::new(move || async move {
                let socket = udp::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
                tokio::time::sleep(Duration::from_millis(1)).await; // to make sure server gets inited
                socket.connect((server_addr, 3000)).await?;
                socket.send(b"ok").await?;
                let mut buf = BytesMut::new();
                socket.recv_buf(&mut buf).await?;
                assert_eq!(b"ok", &buf[..]);
                Ok(())
            }));
            client
                .get()
                .borrow()
                .connect_to_net(nat.get().borrow().lan());
            for _ in 0..10 {
                Sim::tick().unwrap();
            }
            assert!(client.get().borrow().is_idle());
        })
    }

    #[test_log::test]
    fn nat_hole_punch() {
        let s = Sim::new_with_config(Config {
            udp_capacity: 10,
            ip_hop_capacity: 10,
            nic_capacity: 10,
            ..Config::new_with_sync_network()
        });
        s.enter_runtime(|| {
            let net = Sim::add_machine(Network::new_with_ipv4_prefix(
                Ipv4Prefix::new([192, 0, 2, 0].into(), 24).unwrap(),
            ));
            let server = OsShim::new(Host::new(move || async move {
                let socket = udp::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 3000)).await?;
                let mut c1_addr: Option<SocketAddr> = None;
                // every two clients will get paired up with one another
                loop {
                    match c1_addr {
                        Some(c1_addr) => {
                            let mut buf = BytesMut::new();
                            let Ok((_, addr)) = socket.recv_buf_from(&mut buf).await else {
                                continue;
                            };

                            socket.send_to(c1_addr.to_string().as_bytes(), addr).await?;
                            socket.send_to(addr.to_string().as_bytes(), c1_addr).await?;
                        }
                        None => {
                            let mut buf = BytesMut::new();
                            let Ok((_, addr)) = socket.recv_buf_from(&mut buf).await else {
                                continue;
                            };
                            c1_addr = Some(addr);
                        }
                    };
                }
            }));
            let server_addr = Ipv4Addr::from_octets([192, 0, 2, 0]);
            server.get().borrow().connect_to_net(net);
            info!("server address: {server_addr}");
            let nat1 = Sim::add_machine(nat::Nat::<nat::easy::PortRestrictedCone>::new(net));
            trace!("inited server");
            let c1 = OsShim::new(Host::new(move || async move {
                let socket = udp::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
                tokio::time::sleep(Duration::from_millis(1)).await; // to make sure server gets inited
                socket.connect((server_addr, 3000)).await?;
                socket.send(b"hello").await?;
                let mut buf = BytesMut::new();
                socket.recv_buf_from(&mut buf).await?;
                let other_addr: SocketAddr = String::from_utf8_lossy(&buf).parse().unwrap();
                trace!("c1 got other addr! {other_addr}");

                socket.connect(other_addr).await?;
                let mut buf = BytesMut::new();
                socket.send(b"hello").await?;
                socket.send(b"hello").await?;
                socket.recv_buf(&mut buf).await?;

                sleep(Duration::from_millis(50)).await;
                while socket.try_recv(&mut buf).is_ok() {}
                // now we're hole punched
                buf.clear();

                socket.send(b"hello client 2").await?;
                socket.recv_buf(&mut buf).await?;
                assert_eq!(b"hello client 1", &buf[..]);

                Ok(())
            }));
            c1.get().borrow().connect_to_net(nat1.get().borrow().lan());

            let nat2 = Sim::add_machine(nat::Nat::<nat::easy::PortRestrictedCone>::new(net));

            let c2 = OsShim::new(Host::new(move || async move {
                let socket = udp::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
                tokio::time::sleep(Duration::from_millis(1)).await; // to make sure server gets inited
                socket.connect((server_addr, 3000)).await?;
                socket.send(b"hello").await?;
                let mut buf = BytesMut::new();
                socket.recv_buf_from(&mut buf).await?;
                let other_addr: SocketAddr = String::from_utf8_lossy(&buf).parse().unwrap();
                trace!("c2 got other addr! {other_addr}");

                socket.connect(other_addr).await?;
                buf.clear();
                socket.send(b"hello").await?;
                socket.send(b"hello").await?;
                socket.recv_buf(&mut buf).await?;

                sleep(Duration::from_millis(50)).await;
                while socket.try_recv(&mut buf).is_ok() {}
                // now we're hole punched
                buf.clear();
                socket.recv_buf(&mut buf).await?;
                assert_eq!(b"hello client 2", &buf[..]);
                socket.send(b"hello client 1").await?;

                Ok(())
            }));
            c2.get().borrow().connect_to_net(nat2.get().borrow().lan());
            for _ in 0..100 {
                Sim::tick().unwrap();
            }
            assert!(c1.get().borrow().is_idle());
            assert!(c2.get().borrow().is_idle());
        })
    }

    #[test_log::test]
    fn nat_hole_punch_hard_nat() {
        let s = Sim::new_with_config(Config {
            udp_capacity: 10,
            ip_hop_capacity: 10,
            nic_capacity: 10,
            ..Config::new_with_sync_network()
        });
        s.enter_runtime(|| {
            let net = Sim::add_machine(Network::new_with_ipv4_prefix(
                Ipv4Prefix::new([192, 0, 2, 0].into(), 24).unwrap(),
            ));
            let server = OsShim::new(Host::new(move || async move {
                let socket = udp::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 3000)).await?;
                let mut c1_addr: Option<SocketAddr> = None;
                // every two clients will get paired up with one another
                loop {
                    match c1_addr {
                        Some(c1_addr) => {
                            let mut buf = BytesMut::new();
                            let Ok((_, addr)) = socket.recv_buf_from(&mut buf).await else {
                                continue;
                            };

                            socket.send_to(c1_addr.to_string().as_bytes(), addr).await?;
                            socket.send_to(addr.to_string().as_bytes(), c1_addr).await?;
                        }
                        None => {
                            let mut buf = BytesMut::new();
                            let Ok((_, addr)) = socket.recv_buf_from(&mut buf).await else {
                                continue;
                            };
                            c1_addr = Some(addr);
                        }
                    };
                }
            }));
            let server_addr = Ipv4Addr::from_octets([192, 0, 2, 0]);
            server.get().borrow().connect_to_net(net);
            info!("server address: {server_addr}");
            let nat1 = Sim::add_machine(nat::Nat::<nat::hard::Symmetric>::new(net));
            trace!("inited server");
            let c1 = OsShim::new(Host::new(move || async move {
                let socket = udp::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
                tokio::time::sleep(Duration::from_millis(1)).await; // to make sure server gets inited
                socket.connect((server_addr, 3000)).await?;
                socket.send(b"hello").await?;
                let mut buf = BytesMut::new();
                socket.recv_buf_from(&mut buf).await?;
                let other_addr: SocketAddr = String::from_utf8_lossy(&buf).parse().unwrap();
                trace!("c1 got other addr! {other_addr}");

                socket.connect(other_addr).await?;
                let mut buf = BytesMut::new();
                socket.send(b"hello").await?;
                socket.send(b"hello").await?;
                if let Ok(res) =
                    tokio::time::timeout(Duration::from_millis(50), socket.recv_buf(&mut buf)).await
                {
                    res?;
                } else {
                    trace!("timed out receive")
                }

                sleep(Duration::from_millis(50)).await;
                while socket.try_recv(&mut buf).is_ok() {}
                trace!("c1 hole punched");
                buf.clear();

                socket.send(b"hello client 2").await?;
                // wait for send from c2
                sleep(Duration::from_millis(50)).await;
                let res = socket.try_recv_buf(&mut buf);
                // make sure we haven't received the message (got picked up by firewall)
                assert_eq!(res.unwrap_err().kind(), ErrorKind::WouldBlock);

                Ok(())
            }));
            c1.get().borrow().connect_to_net(nat1.get().borrow().lan());

            let nat2 = Sim::add_machine(nat::Nat::<nat::hard::Symmetric>::new(net));

            let c2 = OsShim::new(Host::new(move || async move {
                let socket = udp::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
                tokio::time::sleep(Duration::from_millis(1)).await; // to make sure server gets inited
                socket.connect((server_addr, 3000)).await?;
                socket.send(b"hello").await?;
                let mut buf = BytesMut::new();
                socket.recv_buf_from(&mut buf).await?;
                let other_addr: SocketAddr = String::from_utf8_lossy(&buf).parse().unwrap();
                trace!("c2 got other addr! {other_addr}");

                socket.connect(other_addr).await?;
                buf.clear();
                socket.send(b"hello").await?;
                socket.send(b"hello").await?;
                if let Ok(res) =
                    tokio::time::timeout(Duration::from_millis(50), socket.recv_buf(&mut buf)).await
                {
                    res?;
                } else {
                    trace!("timed out receive")
                }

                sleep(Duration::from_millis(50)).await;
                while socket.try_recv(&mut buf).is_ok() {}
                // now we're hole punched
                trace!("c2 hole punched");
                buf.clear();
                // wait for send from c1
                sleep(Duration::from_millis(50)).await;
                let res = socket.try_recv_buf(&mut buf);
                // make sure we haven't received the message
                assert_eq!(res.unwrap_err().kind(), ErrorKind::WouldBlock);
                socket.send(b"hello client 1").await?;

                Ok(())
            }));
            c2.get().borrow().connect_to_net(nat2.get().borrow().lan());
            for _ in 0..100000 {
                Sim::tick().unwrap();
            }
            // both clients should hang since both are behind a hard nat
            assert!(c1.get().borrow().is_idle());
            assert!(c2.get().borrow().is_idle());
        })
    }
}

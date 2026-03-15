use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Error, ErrorKind},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    ops::{Deref, DerefMut, Range},
    rc::Rc,
};

use bytes::{Bytes, BytesMut};
use scoped_tls::scoped_thread_local;
use tokio::sync::mpsc::{self, Receiver};
use tracing::{info, trace, warn};

use crate::{
    config::CONFIG,
    host::Host,
    net::{
        self,
        ip::{self},
        udp,
    },
    sim::{
        MachineRef, Sim,
        machine::{HasNic, Machine},
    },
};

/// internal send socket
#[derive(Clone)]
struct InnerSendSocket {
    io: mpsc::Sender<udp::Packet>,
}

pub struct OsShim {
    host: Host,
    inner: RefCell<InnerOsShim>,
}

struct InnerOsShim {
    sockets: HashMap<SocketAddr, InnerSendSocket>,
    // need to keep track of for when adding a new network interface
    wildcard_sockets: Vec<u16>,
    nets: HashMap<IpAddr, MachineRef<ip::Network>>,
    port_iter: Range<u16>,
    // for allowing ports to send into the network
    send_incoming: mpsc::Sender<udp::Packet>,
    recv_incoming: RefCell<mpsc::Receiver<udp::Packet>>,
}

impl InnerOsShim {
    fn ephemeral_port_gen() -> Range<u16> {
        49152u16..65535u16
    }
    pub fn new(nets: HashMap<IpAddr, MachineRef<ip::Network>>) -> Self {
        let (send_incoming, recv_incoming) = CONFIG.with(|cfg| mpsc::channel(cfg.nic_capacity()));
        Self {
            wildcard_sockets: Default::default(),
            sockets: Default::default(),
            nets,
            port_iter: Self::ephemeral_port_gen(),
            send_incoming,
            recv_incoming: recv_incoming.into(),
        }
    }

    async fn handle_incoming_ip_packet(&self, packet: Bytes) {
        let udp_packet = match crate::net::udp::Packet::try_from_bytes(packet) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("dropping packet due to malformed udp packet {}", e);
                return;
            }
        };
        let msg_header = udp_packet.header();
        let port = msg_header.get_dst_addr();

        let Some(listener) = self.sockets.get(&port) else {
            tracing::warn!("packet dropped, port {} is not bound", port);
            return;
        };
        listener.io.send(udp_packet).await.unwrap();
    }

    fn send_packet(&self, msg: udp::Packet) -> std::io::Result<()> {
        let mut src_addr = msg.header().get_src_addr();
        let src_ip = src_addr.ip();
        trace!("send_packet sending {:?}", msg.header());

        if src_ip.is_unspecified() {
            for addr in self.nets.keys() {
                let mut msg = msg.clone();
                src_addr.set_ip(*addr);
                msg.set_src_addr(src_addr);
                if let Err(e) = self.send_packet(msg) {
                    info!("error sending packet: {e}");
                }
            }
            return Ok(());
        }
        let mut buf_mut = BytesMut::new();
        msg.write_into_buf(&mut buf_mut);
        let bytes = buf_mut.freeze();
        let Some(net) = self.nets.get(&src_ip) else {
            Err(ErrorKind::NetworkUnreachable)?
        };
        // send it onto the network
        match net.get().borrow().try_send_packet(bytes) {
            Ok(()) => {}
            Err(e) => match e {
                crate::error::Error::PacketParse(_parse_error) => unreachable!(),
                crate::error::Error::Io(error) => Err(error)?,
                crate::error::Error::InvalidPacket(_) => unreachable!(),
            },
        }
        Ok(())
    }

    fn pick_unused_port(&mut self, on: IpAddr) -> u16 {
        while let Some(port) = self.port_iter.next() {
            if !self.port_taken(on, port) {
                return port;
            }
        }
        self.port_iter = Self::ephemeral_port_gen();
        self.pick_unused_port(on)
    }

    fn port_taken(&self, on: IpAddr, port: u16) -> bool {
        // first checks any sockets (if in there then taken everywhere)
        // then checks the specific interface
        if on.is_unspecified() {
            // need to check all interfaces for a conflict
            return self
                .nets
                .keys()
                .any(|v| self.sockets.contains_key(&(*v, port).into()));
        }
        self.sockets.contains_key(&(on, port).into())
    }

    pub fn bind_to_addr(
        &mut self,
        mut addr: SocketAddr,
    ) -> Result<(mpsc::Sender<udp::Packet>, Receiver<udp::Packet>, SocketAddr), std::io::Error>
    {
        if addr.port() == 0 {
            addr.set_port(self.pick_unused_port(addr.ip()));
        }
        let (send, recv) = CONFIG.with(|cfg| mpsc::channel(cfg.udp_capacity()));
        let is = InnerSendSocket { io: send };
        if self.port_taken(addr.ip(), addr.port()) {
            Err(Error::new(
                ErrorKind::AddrInUse,
                format!("address {addr} already in use"),
            ))?;
        }
        if addr.ip().is_unspecified() {
            self.wildcard_sockets.push(addr.port());
            for ip in self.nets.keys() {
                self.sockets.insert((*ip, addr.port()).into(), is.clone());
            }
        } else {
            self.sockets.insert(addr, is);
        }

        Ok((self.send_incoming.clone(), recv, addr))
    }

    pub fn add_net(&mut self, addr: IpAddr, net: MachineRef<ip::Network>) {
        self.nets.insert(addr, net);
    }
}

impl HasNic for OsShim {
    fn nic(&self) -> crate::sim::machine::MachineNic {
        self.host.nic()
    }
}

impl OsShim {
    pub fn new(host: Host) -> MachineRef<Self> {
        let mut ipv4_loopback = net::ip::Network::new();
        ipv4_loopback.add_machine_with_range(&host, "127.0.0.0/8".parse().unwrap());
        let mut ipv6_loopback = net::ip::Network::new();
        ipv6_loopback.add_machine_with_range(&host, "::1/128".parse().unwrap());
        let mut nets = HashMap::new();
        nets.insert(Ipv4Addr::LOCALHOST.into(), Sim::add_machine(ipv4_loopback));
        nets.insert(Ipv6Addr::LOCALHOST.into(), Sim::add_machine(ipv6_loopback));
        let inner = InnerOsShim::new(nets);
        let out = Self {
            host,
            inner: inner.into(),
        };
        let host = out.host();

        // handle receiving a message from the network and forwarding it to the udp port
        host.inner().spawn_local(async move {
            let machine = Sim::get_current_machine::<OsShim>();
            let borrowed = machine.borrow();
            loop {
                let Some(msg) = borrowed.host.inner().read().await else {
                    break;
                };
                borrowed.handle_incoming_ip_packet(msg).await;
            }
            Ok(())
        });

        // handle receiving a message from userspace and forwarding it to the network
        host.inner().spawn_local(async move {
            let machine = Sim::get_current_machine::<OsShim>();
            let borrowed = machine.borrow();
            loop {
                let Some(msg) = borrowed
                    .inner
                    .borrow()
                    .recv_incoming
                    .borrow_mut()
                    .recv()
                    .await
                else {
                    break;
                };
                trace!("sending {:?}", msg.header());
                match borrowed.send_packet(msg) {
                    Ok(_) => {}
                    Err(e) => warn!("error when sending packet: {}", e),
                }
            }
            Ok(())
        });
        drop(host);

        Sim::add_machine(out)
    }

    async fn handle_incoming_ip_packet(&self, packet: Bytes) {
        self.inner.borrow().handle_incoming_ip_packet(packet).await
    }

    pub fn connect_to_net(&self, net: MachineRef<ip::Network>) {
        let addr = net.get().borrow_mut().add_machine(self);
        self.inner.borrow_mut().add_net(addr, net)
    }

    pub fn bind_to_addr(
        &self,
        addr: SocketAddr,
    ) -> Result<(mpsc::Sender<udp::Packet>, Receiver<udp::Packet>, SocketAddr), std::io::Error>
    {
        self.inner.borrow_mut().bind_to_addr(addr)
    }

    pub fn host_mut(&mut self) -> impl DerefMut<Target = Host> {
        &mut self.host
    }

    pub fn host(&self) -> impl Deref<Target = Host> {
        &self.host
    }
    fn send_packet(&self, msg: udp::Packet) -> std::io::Result<()> {
        self.inner.borrow().send_packet(msg)
    }
}

impl Machine for OsShim {
    fn tick(&self, duration: std::time::Duration) -> Result<bool, Box<dyn std::error::Error>> {
        OS.set(&Sim::get_current_machine::<Self>(), || {
            self.host.tick(duration)
        })
    }

    fn id(&self) -> crate::sim::machine::MachineId {
        self.host.id()
    }

    fn is_idle(&self) -> bool {
        // always will be the two tasks for handling sending and receiving messages
        // from the client process.
        self.host.inner().tasks_left() <= 2
    }
}

scoped_thread_local!(pub(crate) static OS: Rc<RefCell<OsShim>>);

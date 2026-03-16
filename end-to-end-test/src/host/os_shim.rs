use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Error, ErrorKind},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    ops::{Deref, DerefMut, Range},
    rc::Rc,
};

use bytes::{Bytes, BytesMut};
use tokio::{
    sync::mpsc::{self, Receiver},
    task::AbortHandle,
};
use tracing::{info, instrument, trace, warn};

use crate::{
    host::Host,
    net::{
        self,
        ip::{self},
        udp,
    },
    sim::{
        MachineRef, Sim,
        config::CONFIG,
        machine::{HasMachineId, HasNic, Machine},
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
    public_ip: Option<IpAddr>,
    ahs: Option<(AbortHandle, AbortHandle)>,
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
            public_ip: None,
            ahs: None,
        }
    }

    pub fn abort(&mut self) {
        let Some((ah1, ah2)) = self.ahs.take() else {
            return;
        };
        ah1.abort();
        ah2.abort();
    }

    pub fn set_abort_handles(&mut self, ahs: (AbortHandle, AbortHandle)) {
        self.ahs = Some(ahs.into());
    }

    pub fn is_aborted(&self) -> bool {
        self.ahs.is_none()
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

        if let Err(e) = listener.io.send(udp_packet).await {
            warn!("error passing packet to listener: {}", e);
        }
    }
    #[instrument(skip(self))]
    fn send_packet(&self, msg: udp::Packet) -> std::io::Result<()> {
        let mut src_addr = msg.header().get_src_addr();
        let src_ip = src_addr.ip();

        if src_ip.is_unspecified() {
            let addr = self.public_ip().unwrap();
            let mut msg = msg.clone();
            src_addr.set_ip(addr);
            msg.set_src_addr(src_addr);
            if let Err(e) = self.send_packet(msg) {
                warn!("error sending packet: {e}");
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

    fn set_public_ip(&mut self, ip: IpAddr) {
        self.public_ip = Some(ip);
    }

    fn public_ip(&self) -> Option<IpAddr> {
        self.public_ip
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

        out.spawn_forwarding_tasks_if_aborted();

        Sim::add_machine(out)
    }

    fn spawn_forwarding_tasks_if_aborted(&self) {
        if !self.inner.borrow().is_aborted() {
            return;
        }

        // handle receiving a message from the network and forwarding it to the udp port
        let ah1 = self.host.inner().spawn_local(async move {
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
        let ah2 = self.host.inner().spawn_local(async move {
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
                match borrowed.send_packet(msg) {
                    Ok(_) => {}
                    Err(e) => warn!("error when sending packet: {}", e),
                }
            }
            Ok(())
        });

        self.inner.borrow_mut().set_abort_handles((ah1, ah2));
    }

    async fn handle_incoming_ip_packet(&self, packet: Bytes) {
        self.inner.borrow().handle_incoming_ip_packet(packet).await
    }

    pub fn connect_to_net(&self, net: MachineRef<ip::Network>) -> IpAddr {
        let addr = net.get().borrow_mut().add_machine(self);
        self.inner.borrow_mut().add_net(addr, net);
        addr
    }

    pub fn set_public_ip(&self, ip: IpAddr) {
        trace!("setting public ip to {ip}");
        self.inner.borrow_mut().set_public_ip(ip);
    }

    pub fn public_ip(&self) -> Option<IpAddr> {
        self.inner.borrow().public_ip()
    }

    pub fn bind_to_addr(
        &self,
        addr: SocketAddr,
    ) -> Result<(mpsc::Sender<udp::Packet>, Receiver<udp::Packet>, SocketAddr), std::io::Error>
    {
        self.inner.borrow_mut().bind_to_addr(addr)
    }

    pub fn start(self) -> MachineRef<OsShim> {
        self.spawn_forwarding_tasks_if_aborted();
        self.host.start();
        let out = Sim::add_machine(self);
        out
    }

    pub fn stop(reference: MachineRef<Self>) -> Self {
        let out = Sim::remove_machine(reference);
        out.inner.borrow_mut().abort();
        out.host.stop();
        out
    }

    fn send_packet(&self, msg: udp::Packet) -> std::io::Result<()> {
        self.inner.borrow().send_packet(msg)
    }
}

impl Machine for OsShim {
    fn basic_machine(&self) -> Rc<crate::sim::machine::BasicMachine> {
        self.host.basic_machine()
    }

    fn is_idle(&self) -> bool {
        // always will be the two tasks for handling sending and receiving messages
        // from the client process.
        self.host.is_idle()
    }
}

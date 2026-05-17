pub mod net;

use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Error, ErrorKind},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    ops::Range,
    rc::Rc,
    time::Duration,
};

use crate::{
    host::{Host, Result},
    net::{
        ip::{self},
        udp,
    },
    sim::{
        MachineIntoRef, MachineRef, Sim,
        config::CONFIG,
        machine::{HasNic, Machine},
    },
};
use bytes::{Bytes, BytesMut};
use tokio::{
    sync::mpsc::{self, Receiver},
    task::AbortHandle,
};
use tracing::{instrument, trace, warn};

/// internal send socket
#[derive(Clone)]
struct InnerSendSocket {
    io: mpsc::Sender<udp::Packet>,
}

struct InnerOsMock {
    sockets: HashMap<SocketAddr, InnerSendSocket>,
    // need to keep track of for when adding a new network interface
    wildcard_sockets: Vec<u16>,
    nets: HashMap<IpAddr, MachineRef<ip::Network>>,
    port_iter: Range<u16>,
    // for allowing ports to send into the network
    send_incoming: mpsc::Sender<udp::Packet>,
    recv_incoming: RefCell<mpsc::Receiver<udp::Packet>>,
    public_ip: Option<(Ipv4Addr, Ipv6Addr)>,
    ahs: Option<(AbortHandle, AbortHandle)>,
}

impl InnerOsMock {
    fn ephemeral_port_gen() -> Range<u16> {
        49152u16..65535u16
    }
    pub fn new(nets: HashMap<IpAddr, MachineRef<ip::Network>>) -> Self {
        let (send_incoming, recv_incoming) = CONFIG.with(|cfg| mpsc::channel(cfg.nic_capacity()));
        Self {
            wildcard_sockets: Vec::default(),
            sockets: HashMap::default(),
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

    pub fn set_abort_handles(&mut self, abort_handles: (AbortHandle, AbortHandle)) {
        self.ahs = Some(abort_handles);
    }

    pub fn is_aborted(&self) -> bool {
        self.ahs.is_none()
    }

    fn handle_incoming_ip_packet(&self, packet: Bytes) {
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
            tracing::warn!(
                "packet dropped, port {} is not bound. Packet {:?}",
                port,
                udp_packet
            );
            return;
        };

        if let Err(e) = listener.io.try_send(udp_packet) {
            warn!("error passing packet to udp listener: {}", e);
        }
    }
    #[instrument(skip(self))]
    fn send_packet(&self, msg: udp::Packet) -> std::io::Result<()> {
        let mut src_addr = msg.header().get_src_addr();
        let src_ip = src_addr.ip();

        if src_ip.is_unspecified() {
            let (v4, v6) = self.public_ips().unwrap();
            let addr: IpAddr = if src_addr.is_ipv4() {
                v4.into()
            } else {
                v6.into()
            };
            let mut msg = msg.clone();
            // do ipv6

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

    fn set_public_ips(&mut self, ip: (Ipv4Addr, Ipv6Addr)) {
        self.public_ip = Some(ip);
    }

    fn public_ips(&self) -> Option<(Ipv4Addr, Ipv6Addr)> {
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

pub struct OsMock {
    host: Host,
    inner: RefCell<InnerOsMock>,
}

impl HasNic for OsMock {
    fn nic(&self) -> crate::sim::machine::MachineNic {
        self.host.nic()
    }
}

impl OsMock {
    pub fn new<F, Fut>(software: F) -> Self
    where
        F: Fn() -> Fut + 'static,
        Fut: Future<Output = Result> + 'static,
    {
        let host = Host::new(software);
        Self::new_with_host(host)
    }
    pub fn new_with_host(host: Host) -> Self {
        let mut ipv4_loopback = ip::Network::new_private_class_c();
        ipv4_loopback.add_machine_with_range(&host, "127.0.0.0/8".parse().unwrap());
        let mut ipv6_loopback = ip::Network::new_private_class_c();
        ipv6_loopback.add_machine_with_range(&host, "::1/128".parse().unwrap());
        let mut nets = HashMap::new();
        nets.insert(Ipv4Addr::LOCALHOST.into(), ipv4_loopback.into_ref());
        nets.insert(Ipv6Addr::LOCALHOST.into(), ipv6_loopback.into_ref());
        let inner = InnerOsMock::new(nets);
        let out = Self {
            host,
            inner: inner.into(),
        };

        out.spawn_forwarding_tasks_if_aborted();

        out
    }

    fn spawn_forwarding_tasks_if_aborted(&self) {
        if !self.inner.borrow().is_aborted() {
            return;
        }

        // handle receiving a message from the network and forwarding it to the udp port
        let ah1 = self.host.inner().spawn_local(async move {
            let machine = Sim::get_current_machine::<OsMock>();
            loop {
                let res = machine.borrow().host.inner().read().await;
                let Some(msg) = res else { break };
                machine.borrow().handle_incoming_ip_packet(msg);
            }
            Ok(())
        });

        // handle receiving a message from userspace and forwarding it to the network
        let ah2 = self.host.inner().spawn_local(async move {
            let machine = Sim::get_current_machine::<OsMock>();
            loop {
                let res = machine
                    .borrow()
                    .inner
                    .borrow()
                    .recv_incoming
                    .borrow_mut()
                    .try_recv();
                let msg = match res {
                    Ok(msg) => msg,
                    Err(e) => match e {
                        mpsc::error::TryRecvError::Empty => {
                            tokio::time::sleep(Duration::from_millis(1)).await;
                            continue;
                        }
                        mpsc::error::TryRecvError::Disconnected => {
                            break;
                        }
                    },
                };
                match machine.borrow().send_packet(msg) {
                    Ok(()) => {}
                    Err(e) => warn!("error when sending packet: {}", e),
                }
            }
            Ok(())
        });

        self.inner.borrow_mut().set_abort_handles((ah1, ah2));
    }

    #[allow(clippy::semicolon_if_nothing_returned)]
    fn handle_incoming_ip_packet(&self, packet: Bytes) {
        self.inner.borrow().handle_incoming_ip_packet(packet)
    }

    pub fn connect_to_net(&self, net: MachineRef<ip::Network>) -> (Ipv4Addr, Ipv6Addr) {
        let addr = net.get().borrow_mut().add_machine(self);
        let mut mut_borrow = self.inner.borrow_mut();
        mut_borrow.add_net(addr.0.into(), net);
        mut_borrow.add_net(addr.1.into(), net);
        if mut_borrow.public_ips().is_none() {
            trace!("set public ip to ({}, {})", addr.0, addr.1);
            mut_borrow.set_public_ips(addr);
        }
        addr
    }

    pub fn connect_to_net_with_ips(
        &self,
        net: MachineRef<ip::Network>,
        addrs: (Ipv4Addr, Ipv6Addr),
    ) {
        net.get()
            .borrow_mut()
            .add_machine_with_range(self, addrs.0.into());
        net.get()
            .borrow_mut()
            .add_machine_with_range(self, addrs.1.into());
        let mut mut_borrow = self.inner.borrow_mut();
        mut_borrow.add_net(addrs.0.into(), net);
        mut_borrow.add_net(addrs.1.into(), net);
        if mut_borrow.public_ips().is_none() {
            trace!("set public ip to ({}, {})", addrs.0, addrs.1);
            mut_borrow.set_public_ips(addrs);
        }
    }

    pub fn connect_to_net_with_ipv4(
        &self,
        net: MachineRef<ip::Network>,
        addr: impl Into<Ipv4Addr>,
    ) {
        let addr = addr.into();
        let addrs = (addr, addr.to_ipv6_mapped());
        self.connect_to_net_with_ips(net, addrs);
    }

    pub fn set_public_ips(&self, ip: (Ipv4Addr, Ipv6Addr)) {
        trace!("setting public IPs to ({}, {})", ip.0, ip.1);
        self.inner.borrow_mut().set_public_ips(ip);
    }

    pub fn public_ips(&self) -> Option<(Ipv4Addr, Ipv6Addr)> {
        self.inner.borrow().public_ips()
    }

    pub fn public_ip_arr(&self) -> Option<[IpAddr; 2]> {
        let (v4, v6) = self.inner.borrow().public_ips()?;
        Some([v4.into(), v6.into()])
    }

    pub fn bind_to_addr(
        &self,
        addr: SocketAddr,
    ) -> Result<(mpsc::Sender<udp::Packet>, Receiver<udp::Packet>, SocketAddr), std::io::Error>
    {
        self.inner.borrow_mut().bind_to_addr(addr)
    }

    pub fn start(&self) {
        self.spawn_forwarding_tasks_if_aborted();
        self.host.start();
    }

    pub fn stop(&self) {
        self.inner.borrow_mut().abort();
        self.host.stop();
    }

    fn send_packet(&self, msg: udp::Packet) -> std::io::Result<()> {
        self.inner.borrow().send_packet(msg)
    }
}

impl Machine for OsMock {
    fn basic_machine(&self) -> Rc<crate::sim::machine::BasicMachine> {
        self.host.basic_machine()
    }

    fn is_idle(&self) -> bool {
        // always will be the two tasks for handling sending and receiving messages
        // from the client process.
        self.host.is_idle()
    }
}

#[cfg(test)]
mod tests {}

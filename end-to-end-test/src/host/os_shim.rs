use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Error, ErrorKind},
    net::{IpAddr, SocketAddr},
    ops::{Deref, DerefMut},
};

use scoped_tls::scoped_thread_local;
use tokio::sync::mpsc::{self, Receiver};

use crate::{
    config::CONFIG,
    host::Host,
    net::{self, ip, udp},
    sim::machine::Machine,
};

struct UdpSendSocket {
    io: mpsc::Sender<udp::Packet>,
}

pub struct OsShim<'a> {
    host: Host<'a>,
    sockets: HashMap<SocketAddr, UdpSendSocket>,
    // from local_addr to net
    loopback: ip::Network,
    nets: HashMap<IpAddr, ip::Network>,
}

impl<'a> OsShim<'a> {
    pub fn new(host: Host<'a>) -> Self {
        let mut loopback = net::ip::Network::new();
        loopback.add_machine(&host, Some("127.0.0.0/8".parse().unwrap()));
        loopback.add_machine(&host, Some("::1/128".parse().unwrap()));
        Self {
            host,
            loopback,
            sockets: Default::default(),
            nets: Default::default(),
        }
    }

    pub fn bind_to_addr(&mut self, addr: SocketAddr) -> std::io::Result<Receiver<udp::Packet>> {
        if self.sockets.contains_key(&addr) {
            Err(Error::new(
                ErrorKind::AddrInUse,
                format!("address {addr} already in use"),
            ))?;
        }
        let (send, recv) = CONFIG.with(|cfg| mpsc::channel(cfg.udp_capacity()));
        self.sockets
            .insert(addr, UdpSendSocket { io: send.clone() });
        Ok(recv)
    }

    pub fn host_mut(&mut self) -> impl DerefMut<Target = Host<'a>> {
        &mut self.host
    }

    pub fn host(&self) -> impl Deref<Target = Host<'a>> {
        &self.host
    }
}

impl Machine for OsShim<'_> {
    fn tick(&self, duration: std::time::Duration) -> Result<bool, Box<dyn std::error::Error>> {
        self.host.tick(duration)
    }

    fn id(&self) -> &crate::sim::machine::MachineId {
        self.host.id()
    }
}

scoped_thread_local!(pub(crate) static OS: RefCell<OsShim>);

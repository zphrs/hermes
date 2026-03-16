use bytes::Bytes;
pub use ip_network::IpNetwork as IpPrefix;
use ip_network_table::IpNetworkTable;
use std::{collections::HashMap, io::ErrorKind, net::IpAddr};

use crate::{
    error::Error,
    net::{self},
    sim::machine::{HasNic, Machine, MachineId},
};
#[derive(Default)]
pub struct Network {
    network: net::Network,
    bound_ips: IpNetworkTable<MachineId>,
    machine_to_prefix: HashMap<MachineId, IpPrefix>,
    ip_generator: crate::net::ip::Generator,
}

impl Network {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn set_host_ip_range(&mut self, host: &MachineId, new_range: IpPrefix) {
        if let Some(prefix) = self.machine_to_prefix.get(host) {
            self.bound_ips.remove(*prefix);
        };
        self.machine_to_prefix.insert(*host, new_range);
        self.bound_ips.insert(new_range, *host);
    }

    pub fn add_machine_with_range(&mut self, host: &impl HasNic, addr: IpPrefix) {
        self.network.add_machine(host);
        // will simply assign a single address if unspecified
        self.bound_ips.insert(addr, host.id());
    }

    /// assigns a single address if unspecified
    pub fn add_machine(&mut self, host: &impl HasNic) -> IpAddr {
        self.network.add_machine(host);
        let generated_ip = self.ip_generator.next();

        self.bound_ips
            .insert(IpPrefix::from(generated_ip), host.id());

        generated_ip
    }

    pub fn machine_ip_prefix(&mut self, id: &MachineId) -> Option<&IpPrefix> {
        self.machine_to_prefix.get(id)
    }

    pub fn network_mut(&mut self) -> &mut net::Network {
        &mut self.network
    }

    pub fn try_send_packet(&self, bytes: Bytes) -> Result<(), Error> {
        let mut cloned_bytes = bytes.clone();
        let ip_header = net::ip::Header::try_from_buf(&mut cloned_bytes)?;
        let dst_addr = ip_header.get_ip_addrs().1;

        let longest_match = self
            .bound_ips
            .longest_match(dst_addr)
            .ok_or(std::io::Error::from(ErrorKind::HostUnreachable))?;
        self.network.try_send_to_host(longest_match.1, bytes)?;

        Ok(())
    }
}

impl Machine for Network {
    fn basic_machine(&self) -> std::rc::Rc<crate::sim::machine::BasicMachine> {
        self.network.basic_machine()
    }

    fn is_idle(&self) -> bool {
        self.network.is_idle()
    }
}

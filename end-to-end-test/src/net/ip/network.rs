use bytes::Bytes;
pub use ip_network::IpNetwork as IpPrefix;
use ip_network_table::IpNetworkTable;
use std::{collections::HashMap, io::ErrorKind, ops::Deref};

use crate::{
    error::Error,
    net::{self, ip},
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

    pub fn add_machine(&mut self, host: &dyn HasNic, addr: Option<IpPrefix>) {
        self.network.add_machine(host);
        // will simply assign an address if unspecified
        self.bound_ips.insert(
            addr.unwrap_or_else(|| IpPrefix::from(self.ip_generator.next())),
            *host.id(),
        );
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
        let ip::Header::V4 { dst, .. } = ip_header else {
            Err(Error::InvalidPacket("should be an ipv4 header"))?
        };

        let longest_match = self
            .bound_ips
            .longest_match(dst)
            .ok_or(std::io::Error::from(ErrorKind::HostUnreachable))?;

        self.network.try_send_to_host(longest_match.1, bytes)?;

        Ok(())
    }
}

impl Machine for Network {
    fn tick(&self, duration: std::time::Duration) -> Result<bool, Box<dyn std::error::Error>> {
        self.network.tick(duration)
    }

    fn id(&self) -> &MachineId {
        self.network.id()
    }
}

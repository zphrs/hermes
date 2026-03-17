use bytes::Bytes;
pub use ip_network::IpNetwork as IpPrefix;
use ip_network_table::IpNetworkTable;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    io::ErrorKind,
    net::IpAddr,
    rc::Rc,
};

use crate::{
    error::Error,
    net::{self},
    sim::{
        MachineRef,
        machine::{HasNic, Machine, MachineId},
    },
};

/// A guard that holds a network partition and removes it when dropped
pub struct PartitionGuard {
    mref: Option<MachineRef<Network>>,
    from: IpAddr,
    to: IpAddr,
}

impl PartitionGuard {
    /// Creates a new partition guard
    fn new(mref: MachineRef<Network>, from: IpAddr, to: IpAddr) -> Self {
        PartitionGuard {
            mref: Some(mref),
            from,
            to,
        }
    }
}

impl Drop for PartitionGuard {
    fn drop(&mut self) {
        if let Some(mref) = self.mref.take() {
            mref.get()
                .borrow_mut()
                .partitions
                .remove(&(self.from, self.to));
        }
    }
}

#[derive(Default)]
pub struct Network {
    network: net::Network,
    bound_ips: IpNetworkTable<MachineId>,
    machine_to_prefix: HashMap<MachineId, IpPrefix>,
    ip_generator: crate::net::ip::Generator,
    // blocking, blocked
    partitions: HashSet<(IpAddr, IpAddr)>,
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

    pub fn add_one_way_partition(
        net: MachineRef<Self>,
        from: IpAddr,
        to: IpAddr,
    ) -> PartitionGuard {
        net.get().borrow_mut().partitions.insert((from, to));
        PartitionGuard::new(net, from, to)
    }

    pub fn add_two_way_partition(
        net: MachineRef<Self>,
        from: IpAddr,
        to: IpAddr,
    ) -> (PartitionGuard, PartitionGuard) {
        let guard1 = Self::add_one_way_partition(net.clone(), from, to);
        let guard2 = Self::add_one_way_partition(net, to, from);
        (guard1, guard2)
    }

    pub fn try_send_packet(&self, bytes: Bytes) -> Result<(), Error> {
        let mut cloned_bytes = bytes.clone();
        let ip_header = net::ip::Header::try_from_buf(&mut cloned_bytes)?;
        let dst_addr = ip_header.get_ip_addrs().1;

        if self.partitions.contains(&ip_header.get_ip_addrs()) {
            tracing::warn!("Packet blocked due to network partition");
            return Ok(());
        }

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

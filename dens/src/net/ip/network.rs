use bytes::Bytes;
pub use ip_network::Ipv4Network as Ipv4Prefix;

use ip_network::{
    IpNetwork as IpPrefix, Ipv6Network,
    iterator::{Ipv4NetworkIterator, Ipv6NetworkIterator},
};
use ip_network_table::IpNetworkTable;
use std::{
    collections::{HashMap, HashSet},
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
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

pub struct Network {
    network: net::Network,
    bound_ips: IpNetworkTable<MachineId>,
    machine_to_prefix: HashMap<MachineId, IpPrefix>,
    ipv4_generator: Ipv4NetworkIterator,
    ipv6_generator: Ipv6NetworkIterator,
    // blocking, blocked
    partitions: HashSet<(IpAddr, IpAddr)>,
}

impl Default for Network {
    fn default() -> Self {
        let local_net_ip = Ipv4Addr::from_octets([192, 168, 0, 0]);
        let net = Ipv4Prefix::new(local_net_ip, 16).unwrap();
        Self {
            ipv4_generator: Ipv4NetworkIterator::new(net, 32),
            ipv6_generator: Ipv6NetworkIterator::new(
                Ipv6Network::new(net.network_address().to_ipv6_mapped(), const { 128 - 16 })
                    .unwrap(),
                128,
            ),
            network: Default::default(),
            bound_ips: Default::default(),
            machine_to_prefix: Default::default(),
            partitions: Default::default(),
        }
    }
}

impl Network {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn new_with_ipv4_prefix(net: Ipv4Prefix) -> Self {
        Self {
            ipv4_generator: Ipv4NetworkIterator::new(net, 32),
            ipv6_generator: Ipv6NetworkIterator::new(
                Ipv6Network::new(
                    net.network_address().to_ipv6_mapped(),
                    128 - (32 - net.netmask()),
                )
                .unwrap(),
                128,
            ),
            network: Default::default(),
            bound_ips: Default::default(),
            machine_to_prefix: Default::default(),
            partitions: Default::default(),
        }
    }

    pub fn set_host_ip_range(&mut self, host: &MachineId, new_range: IpPrefix) {
        if let Some(prefix) = self.machine_to_prefix.get(host) {
            self.bound_ips.remove(*prefix);
        };
        self.machine_to_prefix.insert(*host, new_range);
        self.bound_ips.insert(new_range, *host);
    }
    /// sends all requests addressed towards the host through here
    pub fn add_machine_with_range(&mut self, host: &impl HasNic, addr: IpPrefix) {
        self.network.add_machine(host);
        // will simply assign a single address if unspecified
        self.bound_ips.insert(addr, host.id());
    }

    /// assigns a single address for the machine in the 192.168.0.0/16 range
    pub fn add_machine(&mut self, host: &impl HasNic) -> (Ipv4Addr, Ipv6Addr) {
        self.network.add_machine(host);
        let generated_ipv4 = self
            .ipv4_generator
            .next()
            .expect("shouldn't run out of ip addresses");
        let generated_ipv6 = self
            .ipv6_generator
            .next()
            .expect("shouldn't run out of ip addresses");

        self.bound_ips
            .insert(IpPrefix::from(generated_ipv4), host.id());

        self.bound_ips
            .insert(IpPrefix::from(generated_ipv6), host.id());

        (
            generated_ipv4.network_address(),
            generated_ipv6.network_address(),
        )
    }

    pub fn remove_machine(&mut self, host: &impl HasNic, addr: IpPrefix) {
        self.network.remove_machine(host);
        self.bound_ips.remove(addr);
    }

    pub fn machine_ip_prefix(&mut self, id: &MachineId) -> Option<&IpPrefix> {
        self.machine_to_prefix.get(id)
    }

    pub fn network_mut(&mut self) -> &mut net::Network {
        &mut self.network
    }

    pub fn add_one_way_partition<
        FromAddr: ToOwned<Owned = IpAddr>,
        ToAddr: ToOwned<Owned = IpAddr>,
    >(
        net: MachineRef<Self>,
        froms: impl IntoIterator<Item = FromAddr>,
        tos: impl IntoIterator<Item = ToAddr, IntoIter = impl Iterator<Item = ToAddr> + Clone>,
    ) -> Vec<PartitionGuard> {
        let tos_iter = tos.into_iter();
        froms
            .into_iter()
            .flat_map(move |from| {
                tos_iter.clone().map(move |to| {
                    let to = to.to_owned();
                    let from = from.to_owned();
                    net.get()
                        .borrow_mut()
                        .partitions
                        .insert((from.to_owned(), to));
                    PartitionGuard::new(net, from, to)
                })
            })
            .collect()
    }

    pub fn add_two_way_partition(
        net: MachineRef<Self>,
        from: IpAddr,
        to: IpAddr,
    ) -> (Vec<PartitionGuard>, Vec<PartitionGuard>) {
        let guard1 = Self::add_one_way_partition(net.clone(), [from], [to]);
        let guard2 = Self::add_one_way_partition(net, [to], [from]);
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

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use expect_test::expect;
    use ip_network::{
        Ipv4Network, Ipv6Network,
        iterator::{Ipv4NetworkIterator, Ipv6NetworkIterator},
    };

    #[test]
    fn net_iters() {
        let mut iter =
            Ipv4NetworkIterator::new(Ipv4Network::new([192, 168, 0, 0].into(), 16).unwrap(), 32);
        expect!["192.168.0.0"].assert_eq(&iter.next().unwrap().network_address().to_string());
        expect!["192.168.0.1"].assert_eq(&iter.next().unwrap().network_address().to_string());

        let mut iter = Ipv6NetworkIterator::new(
            Ipv6Network::new(
                Ipv4Addr::from([192, 168, 0, 0]).to_ipv6_mapped(),
                const { 128 - 16 },
            )
            .unwrap(),
            128,
        );
        expect!["::ffff:192.168.0.0"]
            .assert_eq(&iter.next().unwrap().network_address().to_string());
        expect!["::ffff:192.168.0.1"]
            .assert_eq(&iter.next().unwrap().network_address().to_string());
    }
}

pub mod easy;
mod edn_nat;
pub mod ein_nat;
mod firewall;
pub mod hard;
mod router;

use std::{hash::Hash, net::SocketAddr};

use firewall::Firewall;
pub use router::Router;

pub trait Mappable: Clone + Copy + Hash + PartialEq + Eq {}

impl<T: Clone + Copy + Hash + PartialEq + Eq> Mappable for T {}

pub struct GenericNat<
    RouterDependence: Mappable + Into<SocketAddr> + From<(SocketAddr, SocketAddr)>,
    FirewallDependence: Mappable + From<std::net::SocketAddr>,
> {
    router: Router<RouterDependence>,
    firewall: Firewall<FirewallDependence>,
}

impl<
    RouterDependence: Mappable + Into<SocketAddr> + From<(SocketAddr, SocketAddr)>,
    FirewallDependence: Mappable + From<std::net::SocketAddr>,
> Default for GenericNat<RouterDependence, FirewallDependence>
{
    fn default() -> Self {
        Self {
            router: Default::default(),
            firewall: Default::default(),
        }
    }
}

impl<
    RouterDependence: Mappable + Into<SocketAddr> + From<(SocketAddr, SocketAddr)>,
    FirewallDependence: Mappable + From<std::net::SocketAddr>,
> GenericNat<RouterDependence, FirewallDependence>
{
    pub fn new() -> Self {
        Default::default()
    }
}

pub(super) trait Map {
    fn external_port_to_internal_addr(&mut self, port: u16, dst: SocketAddr) -> Option<SocketAddr>;
    fn internal_addr_to_external_port(&mut self, src: SocketAddr, dst: SocketAddr) -> u16;
}

impl<
    RouterDependence: Mappable + Into<SocketAddr> + From<(SocketAddr, SocketAddr)>,
    FirewallDependence: Mappable + From<std::net::SocketAddr>,
> Map for GenericNat<RouterDependence, FirewallDependence>
{
    fn external_port_to_internal_addr(&mut self, port: u16, dst: SocketAddr) -> Option<SocketAddr> {
        let out = self.router.external_port_to_internal_addr(port, dst)?;
        self.firewall.check(out, dst).then_some(out)
    }

    fn internal_addr_to_external_port(&mut self, src: SocketAddr, dst: SocketAddr) -> u16 {
        self.firewall.insert(src, dst);
        self.router.internal_addr_to_external_port(src, dst)
    }
}

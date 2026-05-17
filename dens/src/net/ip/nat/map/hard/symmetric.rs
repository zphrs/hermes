use super::super::edn_nat::EdnNatMapping;
use std::net::SocketAddr;

use super::super::GenericNat;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
/// Endpoint socket dependent (esd) firewall
pub struct EsdFirewall(SocketAddr);

impl From<SocketAddr> for EsdFirewall {
    fn from(value: SocketAddr) -> Self {
        Self(value)
    }
}

pub type Symmetric = GenericNat<EdnNatMapping, EsdFirewall>;

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use crate::Sim;

    use super::super::super::Map as _;
    use super::Symmetric;

    #[test]
    fn same_src_same_port() {
        Sim::new().enter_runtime(|| {
            let mut full_cone = Symmetric::default();
            let port = full_cone.internal_addr_to_external_port(
                SocketAddr::from(([192, 168, 0, 1], 3000)),
                SocketAddr::from(([8, 8, 8, 8], 3000)),
            );
            let port2 = full_cone.internal_addr_to_external_port(
                SocketAddr::from(([192, 168, 0, 1], 3000)),
                SocketAddr::from(([6, 6, 6, 6], 3000)),
            );
            assert_ne!(port, port2);
        });
    }

    #[test]
    fn different_src_different_port() {
        Sim::new().enter_runtime(|| {
            let mut full_cone = Symmetric::default();
            let port = full_cone.internal_addr_to_external_port(
                SocketAddr::from(([192, 168, 0, 1], 3000)),
                SocketAddr::from(([8, 8, 8, 8], 3000)),
            );
            let port2 = full_cone.internal_addr_to_external_port(
                SocketAddr::from(([192, 168, 0, 1], 3001)),
                SocketAddr::from(([8, 8, 8, 8], 3000)),
            );
            assert_ne!(port, port2);
        });
    }

    #[test]
    fn get_src_from_port() {
        Sim::new().enter_runtime(|| {
            let mut full_cone = Symmetric::default();
            let port = full_cone.internal_addr_to_external_port(
                SocketAddr::from(([192, 168, 0, 1], 3000)),
                SocketAddr::from(([8, 8, 8, 8], 3000)),
            );
            let src = full_cone
                .external_port_to_internal_addr(port, SocketAddr::from(([8, 8, 8, 8], 3000)))
                .unwrap();
            assert_eq!(src, SocketAddr::from(([192, 168, 0, 1], 3000)));
        });
    }

    #[test]
    fn different_from_ports() {
        Sim::new().enter_runtime(|| {
            let mut full_cone = Symmetric::default();
            let port = full_cone.internal_addr_to_external_port(
                SocketAddr::from(([192, 168, 0, 1], 3000)),
                SocketAddr::from(([8, 8, 8, 8], 3000)),
            );
            assert!(
                full_cone
                    .external_port_to_internal_addr(port, SocketAddr::from(([8, 8, 8, 8], 3001)))
                    .is_none()
            );
        });
    }

    #[test]
    fn different_from_ips() {
        Sim::new().enter_runtime(|| {
            let mut full_cone = Symmetric::default();
            let port = full_cone.internal_addr_to_external_port(
                SocketAddr::from(([192, 168, 0, 1], 3000)),
                SocketAddr::from(([8, 8, 8, 8], 3000)),
            );
            assert!(
                full_cone
                    .external_port_to_internal_addr(port, SocketAddr::from(([4, 4, 4, 4], 3000)))
                    .is_none()
            );
        });
    }
}

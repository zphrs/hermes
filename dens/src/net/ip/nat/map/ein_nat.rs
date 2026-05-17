use std::net::SocketAddr;

/// endpoint independent NAT (EIN) mapping.
/// Means that changes to destination address doesn't change the
/// assigned port.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct EinNatMapping(SocketAddr);

impl From<EinNatMapping> for SocketAddr {
    fn from(value: EinNatMapping) -> Self {
        value.0
    }
}

impl From<(SocketAddr, SocketAddr)> for EinNatMapping {
    fn from((src, _): (SocketAddr, SocketAddr)) -> Self {
        Self(src)
    }
}

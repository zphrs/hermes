use std::net::SocketAddr;

/// endpoint dependent NAT (EDN) mapping.
/// Means that changes to destination address doesn't change the
/// assigned port.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct EdnNatMapping(SocketAddr, SocketAddr);

impl Into<SocketAddr> for EdnNatMapping {
    fn into(self) -> SocketAddr {
        self.0
    }
}

impl From<(SocketAddr, SocketAddr)> for EdnNatMapping {
    fn from((src, dst): (SocketAddr, SocketAddr)) -> Self {
        Self(src, dst)
    }
}

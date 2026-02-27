pub mod rpc;

use std::{
    hash::Hash,
    net::SocketAddr,
    ops::{AddAssign, Deref, DerefMut},
    sync::OnceLock,
    time::Instant,
};

use kademlia::{HasId, Id};
use sha2::{Digest, Sha256};

struct DefaultInstant(pub Instant);

impl Default for DefaultInstant {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl From<Instant> for DefaultInstant {
    fn from(value: Instant) -> Self {
        Self(value)
    }
}

impl Deref for DefaultInstant {
    type Target = Instant;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DefaultInstant {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct SkyNode {
    #[n(0)]
    address: SocketAddr,
    #[cbor(skip)]
    id: OnceLock<Id<32>>,
    #[cbor(skip)]
    last_heard_at: DefaultInstant,
}

impl AddAssign for SkyNode {
    fn add_assign(&mut self, rhs: Self) {
        assert_eq!(
            self.address, rhs.address,
            "two SkyNodes are only additive if they share the same id"
        );
        if *rhs.last_heard_at > *self.last_heard_at {
            self.last_heard_at = rhs.last_heard_at;
        }
    }
}

// ensures that sky node listening on different ports with same computer are
// considered equal
impl PartialEq for SkyNode {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for SkyNode {}

impl Hash for SkyNode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl From<SocketAddr> for SkyNode {
    fn from(value: SocketAddr) -> Self {
        Self {
            address: value,
            id: Default::default(),
            last_heard_at: Default::default(),
        }
    }
}

impl HasId<32> for SkyNode {
    fn id(&self) -> &Id<32> {
        self.id.get_or_init(|| {
            let bytes = match self.address.ip().to_canonical() {
                std::net::IpAddr::V4(ipv4_addr) => Sha256::digest(ipv4_addr.octets()),
                std::net::IpAddr::V6(ipv6_addr) => Sha256::digest(ipv6_addr.octets()),
            };
            Id::<32>::from(&*bytes)
        })
    }
}

#[cfg(test)]
mod tests {

    use std::net::SocketAddr;

    use expect_test::expect;

    use crate::SkyNode;

    #[test]
    fn test_ser() {
        let encoded = minicbor::to_vec(SkyNode::from(
            "[::7334]:8080".parse::<SocketAddr>().unwrap(),
        ))
        .unwrap();
        expect!["818201825000000000000000000000000000007334191f90"].assert_eq(
            &encoded
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(""),
        );
    }
}

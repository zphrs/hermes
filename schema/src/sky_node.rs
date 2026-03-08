pub mod rpc;

use std::{
    hash::Hash,
    net::{IpAddr, Ipv6Addr},
    ops::{AddAssign, Deref, DerefMut},
    sync::OnceLock,
    time::Instant,
};

use kademlia::{HasId, Id};
use maxlen::MaxLen;
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
    address: IpAddr,
    #[cbor(skip)]
    id: OnceLock<SkyId>,
    #[cbor(skip)]
    last_heard_at: DefaultInstant,
}
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, PartialEq, Eq, MaxLen)]
#[cbor(transparent)]
pub struct SkyId(#[n(0)] pub(crate) Id<32>);

impl From<IpAddr> for SkyId {
    fn from(value: IpAddr) -> Self {
        let bytes = match value.to_canonical() {
            std::net::IpAddr::V4(ipv4_addr) => Sha256::digest(ipv4_addr.octets()),
            std::net::IpAddr::V6(ipv6_addr) => Sha256::digest(ipv6_addr.octets()),
        };
        Self(Id::<32>::from(&*bytes))
    }
}

impl MaxLen for SkyNode {
    fn biggest_instantiation() -> Self {
        Self {
            address: Ipv6Addr::UNSPECIFIED.into(),
            id: OnceLock::new(),
            last_heard_at: Default::default(),
        }
    }
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

impl From<IpAddr> for SkyNode {
    fn from(value: IpAddr) -> Self {
        Self {
            address: value,
            id: Default::default(),
            last_heard_at: Default::default(),
        }
    }
}

impl HasId<32> for SkyNode {
    fn id(&self) -> &Id<32> {
        &self.id.get_or_init(|| SkyId::from(self.address)).0
    }
}

#[cfg(test)]
mod tests {

    use std::net::IpAddr;

    use expect_test::expect;

    use crate::SkyNode;

    #[test]
    fn test_ser() {
        let encoded = minicbor::to_vec(SkyNode::from("::7334".parse::<IpAddr>().unwrap())).unwrap();
        expect!["8182015000000000000000000000000000007334"].assert_eq(
            &encoded
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(""),
        );
    }
}

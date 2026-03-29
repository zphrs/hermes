pub mod rpc;

use std::{
    fmt::Debug,
    hash::Hash,
    net::{IpAddr, SocketAddr},
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, OnceLock},
};

use kademlia::{HasId, Id};
use maxlen::MaxLen;
use sha2::{Digest, Sha256};
use tokio::time::Instant;

// stands for hermes sky
pub const PORT: u16 = u16::from_be_bytes(*b"hs");

#[derive(Clone)]
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

#[derive(Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct SkyNode {
    #[n(0)]
    address: IpAddr,
    #[cbor(skip)]
    id: OnceLock<SkyId>,
    #[cbor(skip)]
    last_reached_at: Arc<Mutex<DefaultInstant>>,
}

impl Debug for SkyNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkyNode")
            .field("address", &self.address)
            .field("id", &self.id())
            .finish()
    }
}

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, PartialEq, Eq, MaxLen,
)]
#[cbor(transparent)]
pub struct SkyId(#[n(0)] pub(crate) Id<32>);

impl std::fmt::Display for SkyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sky{}", self.0)
    }
}

impl SkyId {
    pub fn to_url(&self) -> String {
        let id = self.0.to_hex_string();
        let left = &id[..id.len() / 2];
        let right = &id[id.len() / 2..];
        format!("{left}.{right}.invalid")
    }
    /// # SAFETY
    /// The caller must ensure that the kademlia Id was at one point a SkyId
    /// and was not initially an [`EarthId`](crate::earth_node::EarthId)
    pub unsafe fn from_kademlia_id_unchecked(id: kademlia::Id<32>) -> Self {
        Self(id)
    }
}

impl Into<kademlia::Id<32>> for SkyId {
    fn into(self) -> kademlia::Id<32> {
        self.0
    }
}

impl From<IpAddr> for SkyId {
    fn from(value: IpAddr) -> Self {
        let mut hasher: Sha256 = Sha256::default();
        hasher.update("hermes-sky-id");
        match value.to_canonical() {
            std::net::IpAddr::V4(ipv4_addr) => hasher.update(ipv4_addr.octets()),
            std::net::IpAddr::V6(ipv6_addr) => hasher.update(ipv6_addr.octets()),
        };
        let bytes = hasher.finalize();
        Self(Id::<32>::from(&*bytes))
    }
}

impl MaxLen for SkyNode {
    fn biggest_instantiation() -> Self {
        Self::from(IpAddr::biggest_instantiation())
    }
}

// ensures that sky node listening on different ports with same computer are
// considered equal
impl PartialEq for SkyNode {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
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
            last_reached_at: Default::default(),
        }
    }
}

impl HasId<32> for SkyNode {
    fn id(&self) -> &Id<32> {
        &self.sky_id().0
    }
}

impl SkyNode {
    pub async fn me(get_public_ip: impl FnOnce() -> IpAddr) -> SkyNode {
        let addr = get_public_ip();
        Self::from(addr)
    }
    pub fn sky_id(&self) -> &SkyId {
        &self.id.get_or_init(|| SkyId::from(self.address))
    }

    pub fn ip_address(&self) -> IpAddr {
        self.address
    }

    pub fn socket_address(&self) -> SocketAddr {
        (self.ip_address(), PORT).into()
    }

    pub fn last_reached_at(&self) -> Instant {
        self.last_reached_at.lock().unwrap().0
    }

    pub fn reset_last_reached_at(&self) {
        *self.last_reached_at.lock().unwrap() = Default::default();
    }
}

#[cfg(test)]
mod tests {

    use std::net::IpAddr;

    use expect_test::expect;

    use crate::SkyNode;
    use std::net::Ipv6Addr;

    use crate::sky_node::SkyId;

    #[test]
    pub fn test_fmt() {
        let id = SkyId::from(IpAddr::from(Ipv6Addr::LOCALHOST));

        expect_test::expect!["SkyId(2A6A...8858)"].assert_eq(&format!("{}", id));
    }

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

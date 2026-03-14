pub mod candidate;
pub mod message;
pub mod rpc;

use std::time::Duration;

use crypto_bigint::U256;
use kademlia::Id;
use maxlen::MaxLen;

use crate::sky_node::SkyId;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct EarthNode {
    #[n(0)]
    id: EarthId,
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, PartialEq, Eq, MaxLen)]
#[cbor(transparent)]
pub struct EarthId(#[n(0)] Id<32>);

impl EarthId {
    /// Converts to a sky id to allow comparisons between earth and sky ids. The
    /// result of this transformation changes over time to ensure that earth
    /// nodes change their sky nodes throughout the day.
    /// This is to make targeted denial-of-service attacks from sky nodes
    /// prohibitive, so long as this rotation is combined with honest sky nodes
    /// only announcing new sky nodes to earth nodes after a probationary day of being
    /// consistently online.
    pub fn to_sky_id(&self, since_epoch: Duration) -> SkyId {
        const ONE_DAY: Duration = Duration::from_hours(24);
        let today_offset: U256 = (since_epoch.as_millis() / ONE_DAY.as_millis()).into();
        let offset = today_offset.wrapping_mul(&U256::MAX);
        let id: U256 = U256::from_be_slice(self.0.bytes());
        SkyId(Id::from((id + offset).to_be_bytes()))
    }
}

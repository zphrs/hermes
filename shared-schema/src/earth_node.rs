pub mod candidate;
pub mod message;
pub mod rpc;

use tokio::time::Duration;

use crypto_bigint::{NonZero, U256};
use kademlia::{HasId, HasServerId as _, Id};
use maxlen::MaxLen;
use tracing::debug;

use crate::{SkyNode, sky_node::SkyId};

#[derive(Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct EarthNode {
    #[n(0)]
    id: EarthId,
}

impl EarthNode {
    pub fn new(id: EarthId) -> Self {
        Self { id }
    }

    pub fn earth_id(&self) -> &EarthId {
        &self.id
    }
}

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, PartialEq, Eq, MaxLen,
)]
#[cbor(transparent)]
pub struct EarthId(#[n(0)] Id<32>);

impl EarthId {
    pub const ZERO: EarthId = EarthId(Id::ZERO);

    /// Converts to a sky id to allow comparisons between earth and sky ids. The
    /// result of this transformation changes over time to ensure that earth
    /// nodes change their sky nodes throughout the day.
    /// This is to make targeted denial-of-service attacks from sky nodes
    /// prohibitive, so long as this rotation is combined with honest sky nodes
    /// only announcing new sky nodes to earth nodes after a probationary day of being
    /// consistently online.
    pub fn to_sky_id(&self, since_epoch: Duration) -> SkyId {
        const ONE_DAY: Duration = Duration::from_hours(24);
        let one_day_millis: u128 = ONE_DAY.as_millis();
        let time_within_day: U256 = (since_epoch.as_millis() % one_day_millis).into();
        let step = U256::MAX.wrapping_div(&NonZero::new(U256::from(one_day_millis)).unwrap());
        let offset = step.wrapping_mul(&time_within_day);
        let id: U256 = U256::from_be_slice(self.0.bytes());
        SkyId(Id::from(id.wrapping_add(&offset).to_be_bytes()))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio::time::Duration;

    #[test]
    fn test_to_sky_id() {
        let earth_id = EarthId(Id::from([0u8; 32]));
        let since_epoch = Duration::from_secs(0);
        let sky_id = earth_id.to_sky_id(since_epoch);
        expect_test::expect![[r#"
            SkyId(
                Id(),
            )
        "#]]
        .assert_debug_eq(&sky_id);

        let earth_id = EarthId(Id::from([0u8; 32]));
        let since_epoch = Duration::from_secs(100000);
        let sky_id = earth_id.to_sky_id(since_epoch);
        expect_test::expect![[r#"
            SkyId(
                Id(
                    284BDA12F684BDA1
                    2F684BDA12F684BD
                    A12F684BDA12F684,
                ),
            )
        "#]]
        .assert_debug_eq(&sky_id);
    }
}

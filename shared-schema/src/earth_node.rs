pub mod candidate;
pub mod message;
pub mod rpc;

use tokio::time::Duration;

use crypto_bigint::{Encoding, NonZero, U256};
use kademlia::Id;
use maxlen::MaxLen;

use crate::sky_node::SkyId;

#[derive(Debug, Clone, Eq, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct EarthNode {
    #[n(0)]
    id: EarthId,
}

/// Manually defined because it's likely earth node will get more fields and
/// using ids for equality is fine
impl PartialEq for EarthNode {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl EarthNode {
    pub fn new(id: EarthId) -> Self {
        Self { id }
    }

    pub fn earth_id(&self) -> &EarthId {
        &self.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarthId(U256);

impl MaxLen for EarthId {
    fn biggest_instantiation() -> Self {
        EarthId(U256::MAX)
    }
}

impl<C> minicbor::CborLen<C> for EarthId {
    fn cbor_len(&self, _ctx: &mut C) -> usize {
        self.0.to_be_bytes().len()
    }
}

impl<'b, C> minicbor::Decode<'b, C> for EarthId {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        Ok(Self(U256::from_be_bytes(d.decode_with(ctx)?)))
    }
}

impl<C> minicbor::Encode<C> for EarthId {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.encode_with(self.0.to_be_bytes(), ctx)?;
        Ok(())
    }
}

impl EarthId {
    pub const ZERO: EarthId = EarthId(U256::ZERO);

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
        SkyId(Id::from(self.0.wrapping_add(&offset).to_be_bytes()))
    }

    pub fn from_array(arr: [u8; 32]) -> EarthId {
        Self(U256::from_be_bytes(arr))
    }

    pub fn to_url(&self) -> String {
        let id = format!("{:0x}", self.0);
        let left = &id[..id.len() / 2];
        let right = &id[id.len() / 2..];
        format!("{left}.{right}.earth.hermes.invalid")
    }
}

impl Ord for EarthId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for EarthId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio::time::Duration;

    #[test]
    fn test_to_sky_id() {
        let earth_id = EarthId(U256::from_be_bytes([0u8; 32]));
        let since_epoch = Duration::from_secs(0);
        let sky_id = earth_id.to_sky_id(since_epoch);
        expect_test::expect![[r#"
            SkyId(
                Id(),
            )
        "#]]
        .assert_debug_eq(&sky_id);

        expect_test::expect![
            "00000000000000000000000000000000.00000000000000000000000000000000.sky.hermes.invalid"
        ]
        .assert_eq(&sky_id.to_url());

        let earth_id = EarthId(U256::from_be_bytes([0u8; 32]));
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

        expect_test::expect![
            "284BDA12F684BDA12F684BDA12F684BD.A12F684BDA12F684BDA12F684B122500.sky.hermes.invalid"
        ]
        .assert_eq(&sky_id.to_url());
    }

    #[test]
    fn display_earth_id() {
        let earth_id = EarthId(U256::from_be_bytes([200u8; 32]));
        expect_test::expect![[r#"
            EarthId(
                Uint(0xC8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8C8),
            )
        "#]]
        .assert_debug_eq(&earth_id);

        expect_test::expect![
            r#"c8c8c8c8c8c8c8c8c8c8c8c8c8c8c8c8.c8c8c8c8c8c8c8c8c8c8c8c8c8c8c8c8.earth.hermes.invalid"#
        ]
        .assert_eq(&earth_id.to_url());
    }
}

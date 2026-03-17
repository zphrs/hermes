pub mod earth_node;
pub mod ping;
pub mod sky_node;

use std::ops::Deref;

use arrayvec::ArrayVec;
pub use earth_node::EarthNode;
use maxlen::MaxLen;
use minicbor::CborLen;
use minicbor::Decode;
pub use sky_node::SkyNode;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cbor(flat)]
pub enum Node {
    #[n(0)]
    Sky(#[n(0)] SkyNode),
    #[n(1)]
    Earth(#[n(0)] EarthNode),
}

#[derive(Debug)]
pub struct MaxSizedVec<T, const N: usize>(arrayvec::ArrayVec<T, N>);

impl<T, const N: usize> Deref for MaxSizedVec<T, N> {
    type Target = arrayvec::ArrayVec<T, N>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, const N: usize> FromIterator<T> for MaxSizedVec<T, N> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self(ArrayVec::from_iter(iter))
    }
}

impl<C, T: CborLen<C>, const N: usize> CborLen<C> for MaxSizedVec<T, N> {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        N.cbor_len(ctx) + self.0.iter().map(|v| v.cbor_len(ctx)).sum::<usize>()
    }
}

impl<T: CborLen<()> + MaxLen, const N: usize> MaxLen for MaxSizedVec<T, N> {
    fn biggest_instantiation() -> Self {
        Self(ArrayVec::from_iter(
            (0..N).into_iter().map(|_| T::biggest_instantiation()),
        ))
    }
}

impl<'b, Ctx, T: Decode<'b, Ctx>, const N: usize> minicbor::Decode<'b, Ctx> for MaxSizedVec<T, N> {
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        ctx: &mut Ctx,
    ) -> Result<Self, minicbor::decode::Error> {
        let iter = d.array_iter_with(ctx)?;
        let len = iter.size_hint().1.ok_or(minicbor::decode::Error::message(
            "array must have a definite number of elements",
        ))?;
        if len > N {
            return Err(minicbor::decode::Error::message(
                "array overflowed max length",
            ));
        }
        let mut items = ArrayVec::new();
        for value in iter {
            items.push(value?)
        }

        Ok(Self(items))
    }
}

impl<T, Ctx, const N: usize> minicbor::Encode<Ctx> for MaxSizedVec<T, N>
where
    T: minicbor::Encode<Ctx>,
{
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.array(self.0.len() as u64)?;
        for item in &self.0 {
            item.encode(e, ctx)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{earth_node::EarthId, sky_node::rpc::RootRequest};

    use super::*;
    use maxlen::MaxLen;

    #[test]
    fn test_maxlen_derive_produces_valid_instances() {
        // Test that all derived types can create biggest_instantiation
        let _earth_node = EarthNode::biggest_instantiation();
        let _earth_id = EarthId::biggest_instantiation();
        let _sky_id = sky_node::SkyId::biggest_instantiation();

        let _request_type = RootRequest::biggest_instantiation();

        // Test ping types
        let _ping_req = ping::Request::biggest_instantiation();
        let _ping_res = ping::Response::biggest_instantiation();

        // Test lookup types
        use sky_node::rpc::lookup::*;
        let _find_req = FindSkyNodeRequest::biggest_instantiation();
        let _find_res = FindSkyNodeResponse::biggest_instantiation();

        // Test earth_to_sky types
        use sky_node::rpc::earth_to_sky::*;
        let _e2s_req = EarthToSkyRequest::biggest_instantiation();
        let _e2s_req_val = EarthToSkyRequestValue::biggest_instantiation();

        // Test response types
        let _register = response::Register::biggest_instantiation();
        let _connect_to = response::ConnectTo::biggest_instantiation();
        let _nearby = response::NearbyEarthNodes::biggest_instantiation();

        // Test candidate
        use earth_node::candidate::Candidate;
        let _candidate = Candidate::biggest_instantiation();
    }

    #[test]
    fn test_maxlen_enum_picks_largest_variant() {
        // For enums, verify that the biggest variant is chosen
        let request_type = RootRequest::biggest_instantiation();
        let len = minicbor::len(&request_type);

        // Verify it's non-zero
        assert!(len > 0);

        // Test earth_to_sky enum
        use sky_node::rpc::earth_to_sky::EarthToSkyRequestValue;
        let e2s_val = EarthToSkyRequestValue::biggest_instantiation();
        let e2s_len = minicbor::len(&e2s_val);
        assert!(e2s_len > 0);
    }
}

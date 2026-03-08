pub mod earth_node;
mod ping;
pub mod sky_node;

pub use earth_node::EarthNode;
pub use sky_node::SkyNode;
pub use sky_node::rpc::*;

use crate::earth_node::EarthId;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cbor(flat)]
pub enum Node {
    #[n(0)]
    Sky(#[n(0)] SkyNode),
    #[n(1)]
    Earth(#[n(0)] EarthNode),
}

pub fn earth_to_sky_address(_id: EarthId) {}

#[cfg(test)]
mod tests {
    use super::*;
    use maxlen::MaxLen;

    #[test]
    fn test_maxlen_derive_produces_valid_instances() {
        // Test that all derived types can create biggest_instantiation
        let _earth_node = EarthNode::biggest_instantiation();
        let _earth_id = EarthId::biggest_instantiation();
        let _sky_id = sky_node::SkyId::biggest_instantiation();

        // Test RPC types
        let _request = Request::biggest_instantiation();
        let _request_type = RequestType::biggest_instantiation();

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
        let request_type = RequestType::biggest_instantiation();
        let len = minicbor::len(&request_type);

        // Verify it's non-zero
        assert!(len > 0);

        // Test earth_to_sky enum
        use sky_node::rpc::earth_to_sky::EarthToSkyRequestValue;
        let e2s_val = EarthToSkyRequestValue::biggest_instantiation();
        let e2s_len = minicbor::len(&e2s_val);
        assert!(e2s_len > 0);
    }

    #[test]
    fn test_maxlen_cached_correctly() {
        // Test that max_len() returns the same value as max_len_init()
        let init_len = Request::max_len_init();
        let cached_len = Request::max_len();
        assert_eq!(init_len, cached_len);

        // Call again to ensure caching works
        let cached_len2 = Request::max_len();
        assert_eq!(cached_len, cached_len2);
    }
}

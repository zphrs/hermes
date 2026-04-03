use crate::{
    EarthNode, SkyNode,
    earth_node::{EarthId, candidate::Candidate},
    sky_node::rpc::lookup::FindSkyNodeResponse,
};
use maxlen::MaxLen;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
#[cbor(tag(0))]
pub struct EarthToSkyRequest {
    #[n(0)]
    value: EarthToSkyRequestValue,
    #[n(1)]
    from: EarthNode,
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
#[cbor(flat)]
pub enum EarthToSkyRequestValue {
    #[n(0)]
    Register,
    #[n(1)]
    EarthNodesNear(#[n(0)] EarthId),
    #[n(2)]
    ConnectTo(#[n(0)] EarthNode),
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cbor(flat)]
pub enum KademliaReply<Value> {
    #[n(0)]
    Reply(#[n(0)] Value),
    #[n(1)]
    Redirect(#[n(1)] FindSkyNodeResponse),
}

impl<Value> MaxLen for KademliaReply<Value>
where
    Value: MaxLen + minicbor::CborLen<()> + minicbor::Encode<()>,
{
    fn biggest_instantiation() -> Self {
        let response = Self::Reply(MaxLen::biggest_instantiation());
        let redirect = Self::Redirect(MaxLen::biggest_instantiation());
        if minicbor::len(&response) > minicbor::len(&redirect) {
            response
        } else {
            redirect
        }
    }
}

pub mod response {
    use crate::{EarthNode, SkyNode, earth_node::candidate::Candidate};
    use max_sized_vec::MaxSizedVec;
    use maxlen::MaxLen;

    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct Register {
        #[n(0)]
        pub neighbors: Option<MaxSizedVec<SkyNode, 20>>,
    }

    // either found candidates is valid or not
    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct ConnectTo(#[n(0)] Result<MaxSizedVec<Candidate, 20>, ()>);

    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct NearbyEarthNodes(#[n(0)] MaxSizedVec<EarthNode, 20>);
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct EarthToSkyResponse {
    #[n(0)]
    value: EarthToSkyResponseValue,
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cbor(flat)]
#[cbor(tag(0))]
pub enum EarthToSkyResponseValue {
    /// keeps connection open with ping-pongs
    #[n(0)]
    Register {
        #[n(0)]
        neighbors: Option<[SkyNode; 20]>,
    },
    // either connection is valid or not
    #[n(1)]
    Connection(#[n(0)] Result<[Candidate; 20], ()>),
    /// sent when there are >20 known online sky nodes closer than this sky
    /// node or this sky node is missing the requested information. Connection
    /// to earth node should be `stopped()` after this message.
    #[n(2)]
    NearbyEarthNodes(#[n(0)] max_sized_vec::MaxSizedVec<EarthNode, 20>),
    #[n(3)]
    Redirect(#[n(0)] FindSkyNodeResponse),
}

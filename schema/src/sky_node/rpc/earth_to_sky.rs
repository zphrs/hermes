use crate::{
    EarthNode, SkyNode, earth_node::candidate::Candidate,
    sky_node::rpc::lookup::FindSkyNodeResponse,
};

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct EarthToSkyRequest {
    #[n(0)]
    value: EarthToSkyRequestValue,
    #[n(1)]
    from: EarthNode,
}
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum EarthToSkyRequestValue {
    #[n(0)]
    Register,
    #[n(1)]
    GetNearbyEarthNodes,
    #[n(2)]
    ConnectTo(#[n(0)] EarthNode),
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct EarthToSkyResponse {
    #[n(0)]
    value: EarthToSkyResponseValue,
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
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
    NearbyEarthNodes(#[n(0)] [Option<EarthNode>; 100]),
    #[n(3)]
    Redirect(#[n(0)] FindSkyNodeResponse),
}

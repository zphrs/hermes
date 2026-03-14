pub mod earth_to_sky;
pub mod lookup;
use crate::{
    earth_node::EarthId,
    ping,
    sky_node::rpc::earth_to_sky::{EarthToSkyRequest, EarthToSkyResponse},
};
use maxlen::MaxLen;

use self::lookup::FindSkyNodeRequest;
use self::lookup::FindSkyNodeResponse;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
#[cbor(flat)]
pub enum RootRequest {
    /// can be sent by anyone
    #[n(0)]
    Ping(#[n(0)] ping::Request),
    /// can be sent by anyone, but probably sent by other sky nodes
    #[n(1)]
    FindSkyNode(#[n(0)] FindSkyNodeRequest),
    /// should only be sent by earth nodes
    #[n(2)]
    FromEarth(#[n(0)] EarthToSkyRequest),
    /// find earth nodes
    #[n(3)]
    EarthNodesNear(#[n(0)] EarthId),
}

pub mod response {

    pub use crate::{
        ping::Response as Ping, sky_node::rpc::lookup::FindSkyNodeResponse as FindSkyNode,
    };
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cbor(tag(0))]
pub struct Response {
    #[n(0)]
    value: RootRequest,
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum ResponseType {
    #[n(0)]
    Ping(#[n(0)] ping::Response),
    #[n(1)]
    FindSkyNode(#[n(0)] FindSkyNodeResponse),
    #[n(2)]
    Join(#[n(0)] EarthToSkyResponse),
}

#[cfg(test)]
mod tests {}

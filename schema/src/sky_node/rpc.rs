pub mod earth_to_sky;
pub mod lookup;
use crate::{
    ping,
    sky_node::rpc::earth_to_sky::{EarthToSkyRequest, EarthToSkyResponse},
};

use self::lookup::FindSkyNodeRequest;
use self::lookup::FindSkyNodeResponse;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct Request {
    #[n(0)]
    value: RequestType,
    #[n(1)]
    request_id: u64,
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum RequestType {
    /// can be sent by anyone
    #[n(0)]
    Ping(#[n(0)] ping::Request),
    /// can be sent by anyone, probably sent by other sky nodes
    #[n(1)]
    FindSkyNode(#[n(0)] FindSkyNodeRequest),
    /// should only be sent by earth nodes
    #[n(2)]
    FromEarth(#[n(0)] EarthToSkyRequest),
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct Response {
    #[n(0)]
    value: RequestType,
    #[n(1)]
    request_id: u64,
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

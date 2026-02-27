use crate::ping;
pub mod sky_to_earth;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct Request {
    #[n(0)]
    value: RequestValue,
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum RequestValue {
    #[n(0)]
    FromSky(#[n(0)] sky_to_earth::Request),
    #[n(1)]
    Ping(#[n(0)] ping::Request),
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct Response {
    #[n(0)]
    value: ResponseValue,
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum ResponseValue {
    #[n(0)]
    FromSky(#[n(0)] sky_to_earth::Response),
    #[n(1)]
    Ping(#[n(0)] ping::Response),
}

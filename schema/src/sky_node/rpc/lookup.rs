use crate::{SkyNode, sky_node::SkyId};
use maxlen::MaxLen;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct FindSkyNodeRequest {
    #[n(0)]
    id: SkyId,
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct FindSkyNodeResponse {
    #[n(0)]
    nodes: [SkyNode; 20],
}

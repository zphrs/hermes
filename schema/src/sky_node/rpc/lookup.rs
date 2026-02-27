use kademlia::Id;

use crate::SkyNode;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct FindSkyNodeRequest {
    #[n(0)]
    id: Id<32>,
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct FindSkyNodeResponse {
    #[n(0)]
    nodes: [SkyNode; 20],
}

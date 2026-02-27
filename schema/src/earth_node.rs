pub mod candidate;
pub mod rpc;
pub mod message;

use kademlia::Id;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct EarthNode {
    #[n(0)]
    id: Id<32>,
}

pub mod hour;

use crate::EarthNode;

pub struct Message {}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct MessageHeader {
    #[n(0)]
    to: EarthNode,
    #[n(1)]
    sent_at: hour::SinceEpoch,
}

pub mod authenticate;

use crate::{EarthNode, SkyNode, sky_node::SkyId};
use maxlen::MaxLen;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    maxlen::MaxLen,
)]
#[cbor(flat)]
pub enum Node {
    #[n(0)]
    Sky(#[n(0)] SkyNode),
    #[n(1)]
    Earth(#[n(0)] EarthNode),
}

impl From<SkyNode> for Node {
    fn from(value: SkyNode) -> Self {
        Self::sky(value)
    }
}

impl From<EarthNode> for Node {
    fn from(value: EarthNode) -> Self {
        Self::earth(value)
    }
}

impl Node {
    pub fn sky(sky: SkyNode) -> Self {
        Self::Sky(sky)
    }
    pub fn earth(earth: EarthNode) -> Self {
        Self::Earth(earth)
    }

    pub fn as_sky(&self) -> Option<&SkyNode> {
        match &self {
            Self::Sky(sky_node) => Some(sky_node),
            _ => None,
        }
    }

    pub fn as_earth(&self) -> Option<&EarthNode> {
        match &self {
            Self::Earth(earth) => Some(earth),
            _ => None,
        }
    }

    pub fn into_sky(self) -> Option<SkyNode> {
        match self {
            Node::Sky(sky) => Some(sky),
            _ => None,
        }
    }

    pub fn into_earth(self) -> Option<EarthNode> {
        match self {
            Node::Earth(earth) => Some(earth),
            _ => None,
        }
    }

    pub fn into_sky_id(&self, current_time: SystemTime) -> SkyId {
        match &self {
            Node::Sky(sky_node) => sky_node.sky_id().clone(),
            Node::Earth(earth_node) => earth_node
                .earth_id()
                .to_sky_id(current_time.duration_since(UNIX_EPOCH).unwrap()),
        }
    }
}

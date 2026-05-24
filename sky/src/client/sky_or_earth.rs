use std::time::{SystemTime, UNIX_EPOCH};

use kademlia::HasId;
use shared_schema::{EarthNode, SkyNode};

/// Compatibility layer for client lookups. Allows locating the nearest sky node
/// to a given earth or a sky node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkyOrEarth {
    Sky(SkyNode),
    /// sets its sky id at time of construction
    Earth(EarthNode, kademlia::Id<32>),
}

impl From<SkyNode> for SkyOrEarth {
    fn from(value: SkyNode) -> Self {
        Self::sky(value)
    }
}

impl From<SkyOrEarth> for shared_schema::Node {
    fn from(value: SkyOrEarth) -> Self {
        match value {
            SkyOrEarth::Sky(sky_node) => Self::Sky(sky_node),
            SkyOrEarth::Earth(earth_node, _sky_id) => Self::Earth(earth_node),
        }
    }
}

impl From<(shared_schema::Node, SystemTime)> for SkyOrEarth {
    fn from(value: (shared_schema::Node, SystemTime)) -> Self {
        match value.0 {
            shared_schema::Node::Sky(sky_node) => Self::Sky(sky_node),
            shared_schema::Node::Earth(earth_node) => Self::earth(earth_node, value.1),
        }
    }
}

impl From<(EarthNode, SystemTime)> for SkyOrEarth {
    fn from(value: (EarthNode, SystemTime)) -> Self {
        Self::earth(value.0, value.1)
    }
}

impl SkyOrEarth {
    pub fn sky(sky: SkyNode) -> Self {
        Self::Sky(sky)
    }
    pub fn earth(earth: EarthNode, time: SystemTime) -> Self {
        let id = earth
            .earth_id()
            .to_sky_id(time.duration_since(UNIX_EPOCH).unwrap())
            .into();
        Self::Earth(earth, id)
    }

    pub fn as_sky(&self) -> Option<&SkyNode> {
        match &self {
            Self::Sky(sky_node) => Some(sky_node),
            _ => None,
        }
    }

    pub fn as_earth(&self) -> Option<&EarthNode> {
        match &self {
            Self::Earth(earth, _) => Some(earth),
            _ => None,
        }
    }

    pub fn into_sky(self) -> Option<SkyNode> {
        match self {
            SkyOrEarth::Sky(sky) => Some(sky),
            _ => None,
        }
    }

    pub fn into_earth(self) -> Option<EarthNode> {
        match self {
            SkyOrEarth::Earth(earth, _) => Some(earth),
            _ => None,
        }
    }
}

impl HasId<32> for SkyOrEarth {
    fn id(&self) -> &kademlia::Id<32> {
        match self {
            SkyOrEarth::Sky(sky_node) => sky_node.id(),
            SkyOrEarth::Earth(_, sky_id) => sky_id.id(),
        }
    }
}

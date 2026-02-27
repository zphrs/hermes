mod earth_node;
mod ping;
mod sky_node;

pub use earth_node::EarthNode;
pub use sky_node::SkyNode;
pub use sky_node::rpc::*;
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum Node {
    #[n(0)]
    Sky(#[n(0)] SkyNode),
    #[n(1)]
    Earth(#[n(0)] EarthNode),
}

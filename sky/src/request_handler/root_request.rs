pub use super::FindNodesRequest;
use maxlen::MaxLen;

use shared_schema::ping;

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
#[cbor(flat)]
pub enum RootRequest {
    /// can be sent by anyone
    #[n(0)]
    Ping(#[n(0)] ping::Request),
    /// get nearby known sky nodes based on address
    #[n(1)]
    FindNodes(#[n(0)] FindNodesRequest),
}

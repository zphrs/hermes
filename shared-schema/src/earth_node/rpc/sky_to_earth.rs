use std::time::SystemTime;

use crate::{EarthNode, earth_node::candidate::Candidate};

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct Req {
    #[n(0)]
    value: RequestValue,
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum RequestValue {
    #[n(0)]
    ConnectRequest {
        #[n(0)]
        from: EarthNode,
        #[n(1)]
        conn_candidates: Vec<Candidate>,
        #[n(2)]
        sent_timestamp: SystemTime,
    },
}
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct Res {
    #[n(0)]
    value: ResponseValue,
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum ResponseValue {
    #[n(0)]
    ConnectResponse,
}

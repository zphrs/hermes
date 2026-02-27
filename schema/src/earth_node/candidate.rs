use std::net::SocketAddr;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub enum Candidate {
    #[n(0)]
    UdpSocket(#[n(0)] SocketAddr),
}

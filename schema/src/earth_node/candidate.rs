use maxlen::MaxLen;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
#[cbor(flat)]
pub enum Candidate {
    #[n(0)]
    UdpSocket(#[n(0)] std::net::SocketAddr),
}

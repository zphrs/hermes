use maxlen::MaxLen;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct Request;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct Response;

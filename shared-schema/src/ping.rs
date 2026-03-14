use std::convert::Infallible;

use maxlen::MaxLen;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen, Debug)]
pub struct Request;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen, Debug)]
pub struct Response;

pub struct Method;

impl rpc::Method for Method {
    type Req = Request;

    type Res = Response;

    type Error = Infallible;
}

pub struct Handler {}

impl rpc::Call for Method {
    async fn call(&mut self, _value: Self::Req) -> Result<Self::Res, Self::Error> {
        Ok(Response)
    }
}

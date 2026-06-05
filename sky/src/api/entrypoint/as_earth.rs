use std::convert::Infallible;

use maxlen::MaxLen;
use rpc::MethodWrapper;
use shared_schema::EarthNode;

type LoopbackWrapper = MethodWrapper<super::Method>;

/// Use new to construct this Request. No need to specify a SkyNode since one
/// will be constructed on the server based on the IP address of the sender.
#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct Request(#[n(0)] EarthNode);

impl Request {
    pub fn new(node: EarthNode) -> Self {
        Self(node)
    }
}
#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Response {
    #[n(0)]
    Ok(#[cbor(skip)] MethodWrapper<crate::api::earth_root::Method>),
}

#[derive(Debug)]
pub struct Method;

impl rpc::Method for Method {
    type Req = Request;

    type Res = Response;

    type Error = Infallible;
}

impl rpc::Call for Method {
    async fn call<T: futures_io::AsyncWrite + Unpin + Send + Sync, TransportError>(
        &mut self,
        replier: rpc::Replier<'_, T, Self>,
        _value: Self::Req,
    ) -> Result<rpc::ReplyReceipt<Self::Res>, rpc::ClientError<TransportError, Self::Error>> {
        replier.reply(Response::Ok(MethodWrapper::new())).await
    }
}

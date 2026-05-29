use std::convert::Infallible;

type LoopbackMethod = MethodWrapper<Method>;

use maxlen::MaxLen;
use rpc::MethodWrapper;
#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Request {
    #[n(0)]
    Ping(#[n(0)] shared_schema::ping::Request),
}

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
#[cbor(flat)]
pub enum Response {
    #[n(0)]
    Ping(#[cbor(skip)] LoopbackMethod),
}

impl From<Response> for LoopbackMethod {
    fn from(value: Response) -> Self {
        match value {
            Response::Ping(method_wrapper) => method_wrapper,
        }
    }
}

#[derive(Debug, Clone)]
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
        value: Self::Req,
    ) -> Result<rpc::ReplyReceipt<Self::Res>, rpc::ClientError<TransportError, Self::Error>> {
        match value {
            Request::Ping(_request) => replier.reply(Response::Ping(Default::default())).await,
        }
    }
}

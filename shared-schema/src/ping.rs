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

impl rpc::Call for Method {
    async fn call<T: futures_io::AsyncWrite + Unpin + Send + Sync, TransportError>(
        &mut self,
        replier: rpc::Replier<'_, T, Self>,
        _value: Self::Req,
    ) -> Result<rpc::ReplyReceipt<Self::Res>, rpc::ClientError<TransportError, Self::Error>> {
        replier.reply(Response).await
    }
}

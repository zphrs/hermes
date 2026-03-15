use std::convert::Infallible;

use maxlen::MaxLen;
use rpc::Call;
use shared_schema::ping;

// stands for hermes sky
pub const PORT: u16 = u16::from_be_bytes(*b"hs");

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
#[cbor(flat)]
pub enum Request {
    /// can be sent by anyone
    #[n(0)]
    Ping(#[n(0)] ping::Request),
}

impl From<ping::Request> for Request {
    fn from(value: ping::Request) -> Self {
        Self::Ping(value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}

impl From<Infallible> for Error {
    fn from(_value: Infallible) -> Self {
        unreachable!()
    }
}

pub struct RootHandler {}

impl rpc::RootHandler<Request> for RootHandler {
    type Error = Error;

    async fn handle<T: futures_io::AsyncWrite + Unpin + Sync + Send, TransportError>(
        &mut self,
        root: Request,
        replier: rpc::Replier<'_, T>,
    ) -> Result<rpc::ReplyReceipt, rpc::ClientError<TransportError, Self::Error>> {
        match root {
            Request::Ping(request) => ping::Method.reply(replier, request).await,
        }
    }
}

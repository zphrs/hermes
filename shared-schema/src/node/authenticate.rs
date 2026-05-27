use std::convert::Infallible;

use crate::Node;
use maxlen::MaxLen;

#[derive(Default)]
pub struct Method {
    node: Option<Node>,
}

impl Method {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_node(self) -> Option<Node> {
        self.node
    }
    pub fn node(&self) -> Option<&Node> {
        self.node.as_ref()
    }
}

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
pub struct Request {
    #[n(0)]
    pub node: Node,
}

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
#[cbor(flat)]
pub enum Response {
    #[n(0)]
    Ok,
}

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
        self.node = Some(value.node);
        replier.reply(Response::Ok).await
    }
}

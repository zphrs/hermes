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
    async fn call(&mut self, value: Self::Req) -> Result<Self::Res, Self::Error> {
        self.node = Some(value.node);

        Ok(Response::Ok)
    }
}

impl rpc::RootHandler<Request> for Method {
    type Error = Infallible;

    async fn handle<'a, T: futures_io::AsyncWrite + Unpin + Sync + Send, TransportError: Send>(
        &mut self,
        root: Request,
        replier: rpc::Replier<'a, T>,
    ) -> Result<rpc::ReplyReceipt, rpc::ClientError<TransportError, Self::Error>> {
        replier.reply_with(self, root).await
    }
}

use std::convert::Infallible;

use crate::Node;
use maxlen::MaxLen;
use rpc::traits::method::can_transition;

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

    type CanTransition = can_transition::False;
}

impl rpc::Handler for Method {
    type Error = Infallible;

    async fn handle<Replier: rpc::transport::ReplyHelper<Self>>(
        &mut self,
        replier: Replier,
        value: <Self as rpc::Method>::Req,
    ) -> Result<
        <Replier as rpc::transport::ReplyHelper<Self>>::Receipt<Self>,
        rpc::traits::HandleError<
            <Replier as rpc::transport::ReplyHelper<Self>>::Error,
            <Self as rpc::Handler<Self>>::Error,
        >,
    > {
        self.node = Some(value.node);
        replier.reply(Response::Ok).await
    }
}

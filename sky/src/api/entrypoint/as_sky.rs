use std::{convert::Infallible, net::IpAddr};

use maxlen::MaxLen;
use rpc::MethodWrapper;
use shared_schema::SkyNode;

type LoopbackWrapper = MethodWrapper<Method>;

/// if it is not provided, it will be filled in on the request handling
#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct Request(#[cbor(skip)] Option<SkyNode>);

impl Request {
    pub fn new() -> Self {
        Self(None)
    }
    pub(crate) fn set_sky_node(&mut self, ip: IpAddr) {
        let sky_node = SkyNode::from(ip);
        self.0 = Some(sky_node)
    }
}
#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Response {
    #[n(0)]
    Ok(#[cbor(skip)] MethodWrapper<crate::api::sky_root::Method>),
    #[n(1)]
    Invalid(#[cbor(skip)] LoopbackWrapper),
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
        value: Self::Req,
    ) -> Result<rpc::ReplyReceipt<Self::Res>, rpc::ClientError<TransportError, Self::Error>> {
        match value.0 {
            Some(_) => replier.reply(Response::Ok(Default::default())).await,
            None => replier.reply(Response::Invalid(Default::default())).await,
        }
    }
}

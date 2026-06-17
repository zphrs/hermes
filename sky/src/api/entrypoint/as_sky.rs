use std::{convert::Infallible, net::IpAddr};

use maxlen::MaxLen;
use rpc::traits::{method::can_transition, state};
use shared_schema::SkyNode;

type LoopbackWrapper = state::Wrapper<super::EntrypointState>;

/// Use new to construct this Request. No need to specify a SkyNode since one
/// will be constructed on the server based on the IP address of the sender.
#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct Request(#[cbor(skip)] Option<SkyNode>);

impl Default for Request {
    fn default() -> Self {
        Self(None)
    }
}

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
    Ok(#[cbor(skip)] state::Wrapper<crate::api::sky_root::State>),
    #[n(1)]
    Invalid(#[cbor(skip)] LoopbackWrapper),
}

#[derive(Debug)]
pub struct Method;

impl rpc::Method for Method {
    type Req = Request;

    type Res = Response;

    type CanTransition = can_transition::True;
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
        match value.0 {
            Some(_) => replier.reply(Response::Ok(Default::default())).await,
            None => replier.reply(Response::Invalid(Default::default())).await,
        }
    }
}

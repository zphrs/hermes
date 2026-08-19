use std::{convert::Infallible, net::IpAddr};

use maxlen::MaxLen;
use rpc::{
    method::{Ancestor, is_leaf},
    traits::{method::can_transition, state},
};
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
    Ok(#[n(0)] state::Wrapper<crate::api::sky_root::State>),
    #[n(1)]
    Invalid(#[n(1)] state::Wrapper<super::EntrypointState>),
}

impl state::Has<crate::api::sky_root::State> for Response {
    fn try_extract_wrapper(self) -> Result<rpc::state::Wrapper<crate::api::sky_root::State>, Self> {
        match self {
            Response::Ok(wrapper) => Ok(wrapper),
            other => Err(other),
        }
    }
}

impl state::Has<super::EntrypointState> for Response {
    fn try_extract_wrapper(self) -> Result<rpc::state::Wrapper<super::EntrypointState>, Self> {
        match self {
            Response::Invalid(wrapper) => Ok(wrapper),
            other => Err(other),
        }
    }
}

#[derive(Debug)]
pub struct Method;

impl rpc::Method for Method {
    type Req = Request;

    type Res = Response;

    type CanTransition = can_transition::True;

    type IsLeaf = is_leaf::True;
}

impl<RM: Ancestor<Self>> rpc::Handler<RM> for Method {
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RM, Self>>(
        &mut self,
        replier: Replier,
        value: rpc::ReqOf<Self>,
    ) -> rpc::traits::HandlerResult<RM, Self, Replier, Self::Error> {
        match value.0 {
            Some(_) => {
                let wrapper = replier.new_wrapper();
                replier.reply(Response::Ok(wrapper)).await
            }
            None => {
                let wrapper = replier.new_wrapper();
                replier.reply(Response::Invalid(wrapper)).await
            }
        }
    }
}

use std::convert::Infallible;

use maxlen::MaxLen;
use rpc::{
    method::is_leaf,
    traits::{method::can_transition, state},
};
use shared_schema::EarthNode;

pub type Request = EarthNode;

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Response {
    #[n(0)]
    Ok(#[n(1)] state::Wrapper<crate::api::earth_root::State>),
}

impl state::Has<crate::api::earth_root::State> for Response {
    fn try_extract_wrapper(
        self,
    ) -> Result<rpc::state::Wrapper<crate::api::earth_root::State>, Self> {
        match self {
            Response::Ok(wrapper) => Ok(wrapper),
        }
    }
}

#[derive(Debug)]
pub struct Method;

impl Method {
    pub fn new() -> Self {
        Self
    }
}

impl rpc::Method for Method {
    type Req = Request;

    type Res = Response;

    type CanTransition = can_transition::True;

    type IsLeaf = is_leaf::True;
}

impl<RM: rpc::method::Ancestor<Self>> rpc::Handler<RM> for Method {
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RM, Self>>(
        &mut self,
        replier: Replier,
        _value: rpc::ReqOf<Self>,
    ) -> rpc::traits::HandlerResult<RM, Self, Replier, Self::Error> {
        let wrapper = replier.new_wrapper();
        replier.reply(Response::Ok(wrapper)).await
    }
}

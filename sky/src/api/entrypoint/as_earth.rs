use std::convert::Infallible;

use maxlen::MaxLen;
use rpc::traits::{method::can_transition, state};
use shared_schema::EarthNode;

pub type Request = EarthNode;

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Response {
    #[n(0)]
    Ok(#[cbor(skip)] state::Wrapper<crate::api::earth_root::State>),
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
        replier.reply(Response::Ok(state::Wrapper::new())).await
    }
}

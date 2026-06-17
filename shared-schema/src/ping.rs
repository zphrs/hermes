use std::convert::Infallible;

use maxlen::MaxLen;
use rpc::traits::method::can_transition;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen, Debug)]
pub struct Request;

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen, Debug)]
pub struct Response;

pub struct Method;

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
        _value: <Self as rpc::Method>::Req,
    ) -> Result<
        <Replier as rpc::transport::ReplyHelper<Self>>::Receipt<Self>,
        rpc::traits::HandleError<
            <Replier as rpc::transport::ReplyHelper<Self>>::Error,
            <Self as rpc::Handler<Self>>::Error,
        >,
    > {
        replier.reply(Response).await
    }
}

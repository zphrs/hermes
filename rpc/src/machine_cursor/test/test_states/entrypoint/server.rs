use std::convert::Infallible;

use crate::{state, traits::method::can_transition};

use super::super::server_endpoint;

pub struct Method;

impl crate::Method for Method {
    type Req = super::super::Request;
    // unconditional approval
    type Res = state::Wrapper<server_endpoint::ServerEndpoint>;

    type CanTransition = can_transition::True;
}

impl crate::Handler for Method {
    type Error = Infallible;

    async fn handle<Replier: crate::transport::ReplyHelper<Self>>(
        &mut self,
        replier: Replier,
        value: <Self as crate::Method>::Req,
    ) -> Result<
        <Replier as crate::transport::ReplyHelper<Self>>::Receipt<Self>,
        crate::traits::HandleError<
            <Replier as crate::transport::ReplyHelper<Self>>::Error,
            <Self as crate::Handler<Self>>::Error,
        >,
    > {
        if let Some(sleep) = value.sleep {
            tokio::time::sleep(sleep).await;
        }

        replier.reply(state::Wrapper::new()).await
    }
}

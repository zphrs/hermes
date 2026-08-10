use std::convert::Infallible;

use crate::{method::is_leaf, state, traits::method::can_transition};

use super::super::server_endpoint;

pub struct Method;

impl crate::Method for Method {
    type Req = super::super::Request;
    // unconditional approval
    type Res = state::Wrapper<server_endpoint::ServerEndpoint>;

    type CanTransition = can_transition::True;

    type IsLeaf = is_leaf::True;
}

impl<RM> crate::Handler<RM> for Method
where
    Method: crate::method::ancestor::Leaf<RM>,
{
    type Error = Infallible;

    async fn handle<Replier: crate::ReplyHelper<RM, Self>>(
        &mut self,
        replier: Replier,
        value: crate::ReqOf<Self>,
    ) -> crate::traits::HandlerResult<RM, Self, Replier, Self::Error> {
        if let Some(sleep) = value.sleep {
            tokio::time::sleep(sleep).await;
        }

        let res = replier.new_wrapper();
        replier.reply(res).await
    }
}

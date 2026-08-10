use std::convert::Infallible;

use crate::{
    method::is_leaf,
    state::{self},
    traits::method::can_transition,
};

use super::super::client_endpoint;

pub struct Method;

impl crate::Method for Method {
    type Req = super::super::Request;
    // unconditional approval
    type Res = state::Wrapper<client_endpoint::ClientEndpoint>;

    type CanTransition = can_transition::True;

    type IsLeaf = is_leaf::True;
}

impl<RootMethod> crate::Handler<RootMethod> for Method
where
    Method: crate::method::ancestor::Leaf<RootMethod>,
{
    type Error = Infallible;

    async fn handle<Replier: crate::ReplyHelper<RootMethod, Self>>(
        &mut self,
        replier: Replier,
        value: crate::ReqOf<Self>,
    ) -> crate::traits::HandlerResult<RootMethod, Self, Replier, Self::Error> {
        if let Some(sleep) = value.sleep {
            tokio::time::sleep(sleep).await;
        }

        let res = replier.new_wrapper();
        replier.reply(res).await
    }
}

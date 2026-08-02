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

    async fn handle<Replier: crate::transport::ReplyHelper<Self, RootMethod>>(
        &mut self,
        replier: Replier,
        value: <Self as crate::Method>::Req,
    ) -> Result<
        <Replier as crate::transport::ReplyHelper<Self, RootMethod>>::Receipt<Self>,
        crate::traits::HandleError<
            <Replier as crate::transport::ReplyHelper<Self, RootMethod>>::Error,
            <Self as crate::Handler<RootMethod, Self>>::Error,
        >,
    > {
        if let Some(sleep) = value.sleep {
            tokio::time::sleep(sleep).await;
        }

        let res = replier.new_wrapper();
        replier.reply(res).await
    }
}

use std::convert::Infallible;

use rpc::method::{can_transition, is_leaf};

/// Ping Method
#[derive(Clone, Copy)]
pub struct Method;

impl rpc::Method for Method {
    type Req = ();

    type Res = ();

    type CanTransition = can_transition::False;

    type IsLeaf = is_leaf::True;
}

impl<RootMethod: rpc::method::Ancestor<Method>> rpc::Handler<RootMethod> for Method {
    type Error = Infallible;

    async fn handle<Replier: rpc::transport::ReplyHelper<Self, RootMethod>>(
        &mut self,
        replier: Replier,
        _value: <Self as rpc::Method>::Req,
    ) -> Result<Replier::Receipt<Self>, rpc::traits::HandleError<Replier::Error, Self::Error>> {
        replier.reply(()).await
    }
}

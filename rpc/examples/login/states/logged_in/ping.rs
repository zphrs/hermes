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

    fn handle<Replier: rpc::ReplyHelper<RootMethod, Self>>(
        &mut self,
        replier: Replier,
        (): rpc::ReqOf<Self>,
    ) -> impl Future<Output = rpc::traits::HandlerResult<RootMethod, Self, Replier, Self::Error>>
    {
        replier.reply(())
    }
}

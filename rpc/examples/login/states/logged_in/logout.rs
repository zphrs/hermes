use std::convert::Infallible;

use rpc::{
    method::{can_transition, is_leaf},
    state,
};

use crate::states::entrypoint::Entrypoint;

pub struct Method;

impl rpc::Method for Method {
    type Req = ();

    type Res = state::Wrapper<Entrypoint>;

    type CanTransition = can_transition::True;

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
        let wrapper = replier.new_wrapper();
        replier.reply(wrapper)
    }
}

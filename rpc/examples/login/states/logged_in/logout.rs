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

    async fn handle<Replier: rpc::transport::ReplyHelper<Self, RootMethod>>(
        &mut self,
        replier: Replier,
        _value: <Self as rpc::Method>::Req,
    ) -> Result<Replier::Receipt<Self>, rpc::traits::HandleError<Replier::Error, Self::Error>> {
        let wrapper = replier.new_wrapper();
        replier.reply(wrapper).await
    }
}

use std::convert::Infallible;

pub type Req = ();

pub type Res = ();

use rpc::{
    method::{Ancestor, is_leaf},
    traits::method::can_transition,
};

#[derive(Clone, Copy)]
pub struct Method;

impl rpc::Method for Method {
    type Req = ();

    type Res = ();

    type CanTransition = can_transition::False;

    type IsLeaf = is_leaf::True;
}

impl<RM: Ancestor<Self>> rpc::Handler<RM> for Method {
    type Error = Infallible;

    fn handle<Replier: rpc::ReplyHelper<RM, Self>>(
        &mut self,
        replier: Replier,
        (): rpc::ReqOf<Self>,
    ) -> impl Future<Output = rpc::traits::HandlerResult<RM, Self, Replier, Self::Error>> {
        replier.reply(())
    }
}

use std::convert::Infallible;

use rpc::{
    method::{Ancestor, is_leaf},
    traits::method::can_transition,
};
use shared_schema::EarthNode;
use tokio::time::Instant;

use super::OnlineNodes;
use crate::api::earth_root::register::OnlineNode;

use super::{Candidates, JoinReceipt};

pub type Request = Candidates;
pub type Response = JoinReceipt;

pub struct Method<'a> {
    pub(super) remote: &'a EarthNode,
    pub(super) map: &'a OnlineNodes,
}

impl rpc::Method for Method<'_> {
    type Req = Request;

    type Res = Response;

    type CanTransition = can_transition::False;

    type IsLeaf = is_leaf::True;
}

impl<'a, RM: Ancestor<Method<'a>>> rpc::Handler<RM> for Method<'a> {
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RM, Self>>(
        &mut self,
        replier: Replier,
        value: rpc::ReqOf<Self>,
    ) -> rpc::traits::HandlerResult<RM, Self, Replier, Self::Error> {
        self.map.write().insert(
            self.remote.earth_id().clone(),
            OnlineNode {
                remote: self.remote.clone(),
                connection_candidates: value,
                last_seen: Instant::now(),
            },
        );
        replier.reply(JoinReceipt()).await
    }
}

use std::convert::Infallible;

use rpc::traits::method::can_transition;
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
}

impl rpc::Handler for Method<'_> {
    type Error = Infallible;

    async fn handle<Replier: rpc::transport::ReplyHelper<Self>>(
        &mut self,
        replier: Replier,
        value: <Self as rpc::Method>::Req,
    ) -> Result<
        <Replier as rpc::transport::ReplyHelper<Self>>::Receipt<Self>,
        rpc::traits::HandleError<
            <Replier as rpc::transport::ReplyHelper<Self>>::Error,
            <Self as rpc::Handler<Self>>::Error,
        >,
    > {
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

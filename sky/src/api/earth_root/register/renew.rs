use std::convert::Infallible;

use maxlen::MaxLen;
use rpc::{
    method::{Ancestor, is_leaf},
    traits::method::can_transition,
};
use shared_schema::EarthNode;
use tokio::time::Instant;

use super::JoinReceipt;

pub struct Method<'a> {
    pub(super) remote: &'a EarthNode,
    pub(super) map: &'a super::OnlineNodes,
}

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
#[cbor(flat)]
pub enum Response {
    #[n(0)]
    Ok(#[n(0)] JoinReceipt),
    #[n(1)]
    NotFound,
}

pub type Request = JoinReceipt;

impl rpc::Method for Method<'_> {
    type Req = Request;

    type Res = Response;

    type CanTransition = can_transition::False;

    type IsLeaf = is_leaf::True;
}

impl<RM: Ancestor<Self>> rpc::Handler<RM> for Method<'_> {
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RM, Self>>(
        &mut self,
        replier: Replier,
        _value: rpc::ReqOf<Self>,
    ) -> rpc::traits::HandlerResult<RM, Self, Replier, Self::Error> {
        let output = {
            let mut btree_map = self.map.write();
            if let Some(exists) = btree_map.get_mut(self.remote.earth_id()) {
                exists.last_seen = Instant::now();
                Response::Ok(JoinReceipt())
            } else {
                Response::NotFound
            }
        };

        replier.reply(output).await
    }
}

use std::convert::Infallible;

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

    type Error = Infallible;
}

impl rpc::Call for Method<'_> {
    async fn call<T: futures_io::AsyncWrite + Unpin + Send + Sync, TransportError>(
        &mut self,
        replier: rpc::Replier<'_, T, Self>,
        value: Self::Req,
    ) -> Result<rpc::ReplyReceipt<Self::Res>, rpc::ClientError<TransportError, Self::Error>> {
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

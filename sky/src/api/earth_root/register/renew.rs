use std::convert::Infallible;

use maxlen::MaxLen;
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

    type Error = Infallible;
}

impl rpc::Handler for Method<'_> {
    async fn handle<T: futures_io::AsyncWrite + Unpin + Send + Sync, TransportError>(
        &mut self,
        replier: rpc::ImmediateReplier<'_, T, Self>,
        _value: Self::Req,
    ) -> Result<rpc::ReplyReceipt<Self::Res>, rpc::HandleOneRequestError<TransportError, Self::Error>> {
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

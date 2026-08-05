mod query;
pub mod query_owned;

use tracing::debug;

use crate::{
    Caller, CallerError, Method, RpcMessage, method::is_leaf,
    traits::method::not_applicable::NotApplicable, transport::BiStream,
};

pub use query::PendingQuery;
pub use query_owned::PendingQueryOwned;

pub trait CallerExt: Caller {
    fn query<M: Method<IsLeaf = is_leaf::True>, RootReq: RpcMessage>(
        &self,
        req: M::Req,
    ) -> PendingQuery<Self, M, RootReq>
    where
        RootReq: From<M::Req>,
        M::Res: RpcMessage,
    {
        PendingQuery::<Self, M, RootReq>::new(self, req)
    }

    fn query_owned<M: Method<IsLeaf = is_leaf::True>, RootReq: RpcMessage>(
        self,
        req: M::Req,
    ) -> PendingQueryOwned<Self, M, RootReq>
    where
        RootReq: From<M::Req>,
        M::Res: RpcMessage,
    {
        PendingQueryOwned::<Self, M, RootReq>::new(self, req)
    }

    fn notify<M: Method<Res = NotApplicable>, RootReq: RpcMessage>(
        &self,
        req: M::Req,
    ) -> impl Future<Output = Result<(), CallerError<Self::Error>>>
    where
        RootReq: From<M::Req>,
    {
        async {
            let (write, _read) = self.open_stream().await.map_err(CallerError::Transport)?;
            debug!("sending notification");

            {
                let root: RootReq = req.into();
                let mut sender = minicbor_io::AsyncWriter::new(write);
                sender.write(root).await.map_err(CallerError::Minicbor)?;
                // drops write here to indicate no more writes will occur
            }
            debug!("sent notification");

            Ok(())
        }
    }
}

impl<T: BiStream + Caller> CallerExt for T {}

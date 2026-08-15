mod query;
pub mod query_owned;

use tracing::debug;

use crate::{
    Caller, CallerError, Method, RpcMessage,
    method::{FromDescendant, is_leaf},
    traits::method::not_applicable::NotApplicable,
    transport::BiStream,
};

pub use query::PendingQuery;
pub use query_owned::PendingQueryOwned;

pub trait CallerExt: Caller {
    fn query<M: Method<IsLeaf = is_leaf::True>, RootMethod: FromDescendant<M>>(
        &self,
        req: M::Req,
    ) -> PendingQuery<Self, M, RootMethod::Req>
    where
        M::Res: RpcMessage,
    {
        PendingQuery::<Self, M, RootMethod::Req>::new(self, RootMethod::from_descendant_req(req))
    }

    fn query_owned<
        M: Method<IsLeaf = is_leaf::True>,
        RootMethod: crate::method::FromDescendant<M>,
    >(
        self,
        req: M::Req,
    ) -> PendingQueryOwned<Self, M, RootMethod::Req>
    where
        M::Res: RpcMessage,
    {
        PendingQueryOwned::<Self, M, RootMethod::Req>::new(
            self,
            RootMethod::from_descendant_req(req),
        )
    }

    fn notify<RootMethod: FromDescendant<M>, M: Method<Res = NotApplicable>>(
        &self,
        req: M::Req,
    ) -> impl Future<Output = Result<(), CallerError<Self::Error>>>
    where
        crate::ReqOf<RootMethod>: RpcMessage,
    {
        async {
            let (write, _read) = self.open_stream().await.map_err(CallerError::Transport)?;
            debug!("sending notification");

            {
                let root = RootMethod::from_descendant_req(req);
                let mut sender = minicbor_io::AsyncWriter::new(write);
                sender.write(root).await.map_err(CallerError::Minicbor)?;
                // drops write here to indicate no more writes will occur
            }
            debug!("sent notification");

            Ok(())
        }
    }
}

pub(crate) trait PrivateCallerExt: Caller {
    fn query_owned_from_root<M: Method<IsLeaf = is_leaf::True>, RootReq>(
        self,
        root_req: RootReq,
    ) -> PendingQueryOwned<Self, M, RootReq> {
        PendingQueryOwned::new(self, root_req)
    }
}

impl<T: BiStream + Caller> PrivateCallerExt for T {}

impl<T: BiStream + Caller> CallerExt for T {}

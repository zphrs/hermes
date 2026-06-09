use crate::ClientError;
use std::marker::PhantomData;

pub struct ConcurrentRequestHandler<Handler: crate::Call, ParallelHandler: crate::Call> {
    handler: ParallelHandler,
    _marker: PhantomData<Handler>,
}

impl<Handler: crate::Call, ParallelHandler: crate::Call + Clone> Clone
    for ConcurrentRequestHandler<Handler, ParallelHandler>
{
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            _marker: self._marker.clone(),
        }
    }
}

impl<Handler: crate::Call, ParallelHandler: crate::Call> std::fmt::Debug
    for ConcurrentRequestHandler<Handler, ParallelHandler>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConcurrentRequestHandler").finish()
    }
}

impl<Handler: crate::Call, ParallelHandler: crate::Call>
    ConcurrentRequestHandler<Handler, ParallelHandler>
{
    pub fn new(handler: ParallelHandler) -> Self {
        Self {
            handler,
            _marker: PhantomData,
        }
    }
}

impl<Handler: crate::Call, ParallelHandler: crate::Call> crate::Method
    for ConcurrentRequestHandler<Handler, ParallelHandler>
{
    type Req = Handler::Req;

    type Res = ParallelHandler::Res;

    type Error = ConcurrentRequestHandlerError<ParallelHandler::Error, Handler::Req>;
}

impl<Handler: crate::Call, ParallelHandler: crate::Call> crate::Call
    for ConcurrentRequestHandler<Handler, ParallelHandler>
where
    Handler::Req: crate::RpcMessage
        + Send
        + std::marker::Sync
        + From<<ParallelHandler::Req as TryFrom<Handler::Req>>::Error>,
    Handler: crate::Call,
    ParallelHandler: crate::Call + std::marker::Send + Clone,
    ParallelHandler::Req: TryFrom<Handler::Req> + crate::RpcMessage + std::marker::Send,
    ParallelHandler::Error: std::marker::Send,
    ParallelHandler::Res: Sync,
    Handler::Req: From<<ParallelHandler::Req as TryFrom<Handler::Req>>::Error>,
    Self: Send,
{
    async fn call<T: futures::AsyncWrite + Unpin + Send + Sync, TransportError>(
        &mut self,
        replier: crate::Replier<'_, T, Self>,
        value: Self::Req,
    ) -> Result<
        crate::transport::ReplyReceipt<Self::Res>,
        crate::ClientError<TransportError, Self::Error>,
    > {
        let parallel_req = match ParallelHandler::Req::try_from(value) {
            Ok(v) => v,
            Err(root) => {
                return Err(ClientError::App(ConcurrentRequestHandlerError::Root(
                    root.into(),
                )));
            }
        };
        self.handler
            .call(replier.change_method(&parallel_req), parallel_req)
            .await
            .map_err(|e| match e {
                ClientError::Rpc(rpc_error) => ClientError::Rpc(rpc_error),
                ClientError::Transport(tp) => ClientError::Transport(tp),
                ClientError::App(app) => {
                    ClientError::App(ConcurrentRequestHandlerError::ParallelHandler(app))
                }
            })
    }
}
pub enum ConcurrentRequestHandlerError<ParallelError, Root> {
    ParallelHandler(ParallelError),
    Root(Root),
}

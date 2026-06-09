#[cfg(test)]
pub(crate) mod tests;

use std::fmt::Debug;
use std::marker::PhantomData;

use futures::FutureExt as _;
use futures::StreamExt as _;
use futures::select;
use futures::stream::FuturesUnordered;
use maxlen::MaxLen;
use tracing::trace;

use crate::RpcMessage;
use crate::transport::Caller;
use crate::transport::Client;
use crate::transport::ClientError;
use crate::transport::Replier;

pub struct MethodWrapper<Handler: crate::Method>(PhantomData<Handler>);

impl<Handler: crate::Method> Debug for MethodWrapper<Handler> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MethodWrapper").finish()
    }
}

impl<Handler: crate::Method> Default for MethodWrapper<Handler> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<C, Handler: crate::Method> minicbor::CborLen<C> for MethodWrapper<Handler> {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        PhantomData::cbor_len(&self.0, ctx)
    }
}

impl<Handler: crate::Method> MaxLen for MethodWrapper<Handler> {
    fn biggest_instantiation() -> Self {
        Self::default()
    }
}

impl<Handler: crate::Method + std::marker::Send> MethodWrapper<Handler> {
    pub fn new() -> Self {
        Self(PhantomData)
    }

    pub fn handle_state_transition_request<'a, C: crate::transport::Client>(
        self,
        client: &'a C,
        mut stream: (C::SendStream, C::RecvStream),
        handler: &mut Handler,
    ) -> impl Future<Output = Result<Handler::Res, crate::ClientError<C::Error, Handler::Error>>> + Send
    where
        Handler: crate::Call + Send + 'a,
        Handler::Req: crate::RpcMessage,
        Handler::Error: std::marker::Send,
    {
        async move {
            client
                .handle_one_request::<Handler>(&mut stream, handler)
                .await
        }
    }

    pub async fn query_loopback<M: crate::Method, C: Caller>(
        &self,
        req: M::Req,
        caller: &C,
    ) -> Result<M::Res, crate::transport::CallerError<C::Error>>
    where
        Handler: crate::Call,
        Handler::Req: From<M::Req>,
        Handler::Req: crate::RpcMessage + From<Handler::Req> + Send,
        Handler::Req: From<M::Req>,
        Self: From<Handler::Res>,
        M::Res: RpcMessage,
    {
        caller.query::<M, Handler::Req>(req).await
    }

    pub async fn query_loopback_child<M: crate::Method, C: Caller, ParallelHandler>(
        &self,
        req: M::Req,
        caller: &C,
    ) -> Result<M::Res, crate::transport::CallerError<C::Error>>
    where
        ParallelHandler: crate::Call,
        ParallelHandler::Req: From<M::Req>,
        Handler::Req: crate::RpcMessage + From<ParallelHandler::Req> + Send,
        Handler::Req: From<M::Req>,
        Self: From<ParallelHandler::Res>,
        M::Res: RpcMessage,
    {
        caller.query::<M, Handler::Req>(req).await
    }

    pub async fn query<M: crate::Method, C: Caller>(
        self,
        req: M::Req,
        caller: &C,
    ) -> Result<M::Res, crate::transport::CallerError<C::Error>>
    where
        Handler::Req: crate::RpcMessage + From<M::Req> + Send,
        M::Res: RpcMessage,
    {
        caller.query::<M, Handler::Req>(req).await
    }

    pub fn handle_only_concurrent_requests<'a, C: crate::transport::Client>(
        self,
        client: &'a C,
        handler: Handler,
    ) -> impl std::future::Future<
        Output = Result<(), ClientError<<C as Client>::Error, Handler::Error>>,
    > + Send
    where
        Handler::Req: crate::RpcMessage + Send + std::marker::Sync,
        Handler: crate::Call + std::marker::Send + Clone + 'a,
        Handler::Req: TryFrom<Handler::Req> + crate::RpcMessage + std::marker::Send,
        Handler::Error: std::marker::Send,
        Handler::Res: Sync,
        Handler::Error: From<Handler::Error> + From<<Handler::Req as TryFrom<Handler::Req>>::Error>,
    {
        async move {
            let mut js = FuturesUnordered::new();

            loop {
                select! {
                    maybe_stream = client.accept_stream().fuse() => {
                        let mut stream = match maybe_stream {
                            Ok(stream) => stream,
                            Err(e) => return Err(ClientError::Transport(e)),

                        };
                        let mut handler = handler.clone();
                        js.push(async move { client.handle_one_request(&mut stream, &mut handler).await });
                    }
                    next_result = js.select_next_some() => {
                        let _response = next_result?;
                        trace!("successfully replied");
                    }
                };
            }
        }
    }

    pub async fn handle_concurrent_requests<'a, C: crate::transport::Client, ParallelHandler>(
        self,
        client: &'a C,
        parallel_handler: ParallelHandler,
        handler: &'a mut Handler,
    ) -> Result<Handler::Res, ClientError<<C as Client>::Error, Handler::Error>>
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
        Handler::Error: From<ParallelHandler::Error>,
    {
        let mut js = FuturesUnordered::new();

        let concurrent_handler =
            ConcurrentRequestHandler::<Handler, ParallelHandler>::new(parallel_handler);
        loop {
            select! {
                maybe_stream = client.accept_stream().fuse() => {
                    let mut stream = match maybe_stream {
                        Ok(stream) => stream,
                        Err(e) => return Err(ClientError::Transport(e)),

                    };
                    let mut handler = concurrent_handler.clone();
                    js.push(async move {
                        (client.handle_one_request(&mut stream, &mut handler).await, stream)
                    });
                }
                next_result = js.select_next_some() => {
                    match next_result {
                        (Ok(_response), _) => {
                            trace!("successfully replied")
                        },
                        (Err(ClientError::App(ConcurrentRequestHandlerError::Root(root))), stream) => {
                            let mut writer = minicbor_io::AsyncWriter::new(stream.0);
                            // got to non-concurrent value
                            return handler.call(Replier::new(&mut writer), root).await.map(crate::ReplyReceipt::into_inner)
                        }
                        (Err(ClientError::App(ConcurrentRequestHandlerError::ParallelHandler(e))), _) => return Err(ClientError::App(Handler::Error::from(e))),
                        (Err(ClientError::Rpc(rpc_error)), _) => return Err(ClientError::Rpc(rpc_error)),
                        (Err(ClientError::Transport(transport_error)), _) => return Err(ClientError::Transport(transport_error)),

                    };
                }
            };
        }
    }
}

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

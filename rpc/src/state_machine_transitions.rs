#[cfg(test)]
mod tests;

use std::marker::PhantomData;

use futures::FutureExt as _;
use futures::StreamExt as _;
use futures::select;
use futures::stream::FuturesUnordered;
use maxlen::MaxLen;
use tracing::trace;

use crate::transport::Caller;
use crate::transport::Client;
use crate::transport::ClientError;
use crate::transport::Replier;

pub struct RootHandlerWrapper<Root: crate::RpcMessage, Handler: crate::RootHandler<Root>>(
    PhantomData<(Root, Handler)>,
);

impl<'b, C, Root: crate::RpcMessage, Handler: crate::RootHandler<Root>> minicbor::Decode<'b, C>
    for RootHandlerWrapper<Root, Handler>
{
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        PhantomData::decode(d, ctx).map(|pd| RootHandlerWrapper(pd))
    }
}

impl<C, Root: crate::RpcMessage, Handler: crate::RootHandler<Root>> minicbor::Encode<C>
    for RootHandlerWrapper<Root, Handler>
{
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        PhantomData::encode(&self.0, e, ctx)
    }
}

impl<Root: crate::RpcMessage, Handler: crate::RootHandler<Root>> Default
    for RootHandlerWrapper<Root, Handler>
{
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<C, Root: crate::RpcMessage, Handler: crate::RootHandler<Root>> minicbor::CborLen<C>
    for RootHandlerWrapper<Root, Handler>
{
    fn cbor_len(&self, ctx: &mut C) -> usize {
        PhantomData::cbor_len(&self.0, ctx)
    }
}

impl<Root: crate::RpcMessage, Handler: crate::RootHandler<Root>> MaxLen
    for RootHandlerWrapper<Root, Handler>
{
    fn biggest_instantiation() -> Self {
        Self::default()
    }
}

impl<Root: crate::RpcMessage, Handler: crate::RootHandler<Root>> RootHandlerWrapper<Root, Handler> {
    pub async fn handle_state_transition_request<'a, C: crate::transport::Client>(
        self,
        client: &'a C,
        mut stream: (C::SendStream, C::RecvStream),
        handler: &'a mut Handler,
    ) -> Result<Handler::Response, crate::ClientError<C::Error, Handler::Error>>
    where
        Handler: 'a,
        Root: 'a,
    {
        client
            .handle_one_request::<Root, Handler>(&mut stream, handler)
            .await
    }
    pub async fn query<M: crate::Method, C: Caller>(
        self,
        req: M::Req,
        caller: &C,
    ) -> Result<M::Res, crate::transport::CallerError<C::Error>>
    where
        Root: crate::RpcMessage + From<M::Req> + Send,
    {
        caller.query::<M, Root>(req).await
    }

    pub async fn handle_concurrent_requests<
        'a,
        C: crate::transport::Client,
        ParallelRoot,
        ParallelHandler,
    >(
        self,
        client: &'a C,
        parallel_handler: ParallelHandler,
        handler: &'a mut Handler,
    ) -> Result<
        <Handler as crate::RootHandler<Root>>::Response,
        ClientError<<C as Client>::Error, <Handler as crate::RootHandler<Root>>::Error>,
    >
    where
        Root: crate::RpcMessage
            + Send
            + std::marker::Sync
            + From<<ParallelRoot as TryFrom<Root>>::Error>,
        Handler: crate::RootHandler<Root>,
        ParallelHandler: crate::RootHandler<ParallelRoot> + std::marker::Send + Clone,
        ParallelRoot: TryFrom<Root> + crate::RpcMessage + std::marker::Send,
        <ParallelHandler as crate::RootHandler<ParallelRoot>>::Error: std::marker::Send,
        <ParallelHandler as crate::RootHandler<ParallelRoot>>::Response: Sync,
    {
        let mut js = FuturesUnordered::new();

        let concurrent_handler =
            ConcurrentRequestHandler::<Root, Handler, ParallelRoot, ParallelHandler>::new(
                parallel_handler,
            );
        loop {
            select! {
                maybe_stream = client.accept_stream().fuse() => {
                    let mut stream = match maybe_stream {
                        Ok(stream) => stream,
                        Err(e) => return Err(ClientError::Transport(e)),

                    };
                    let mut handler = concurrent_handler.clone();
                    js.push(async move { (client.handle_one_request(&mut stream, &mut handler).await, stream) });
                }
                next_result = js.select_next_some() => {
                    match next_result {
                        (Ok(Ok(_response)), _) => {
                            trace!("successfully replied")
                        },
                        (Ok(Err(root)), stream) => {
                            let mut writer = minicbor_io::AsyncWriter::new(stream.0);
                            // got to non-concurrent value
                            return handler.handle(root, Replier::new(&mut writer)).await.map(crate::ReplyReceipt::into_inner)
                        }
                        (Err(e), _) => {
                            return Err(match e {
                                ClientError::Rpc(rpc_error) => ClientError::Rpc(rpc_error),
                                ClientError::Transport(tp) => ClientError::Transport(tp),
                                ClientError::App(e) => ClientError::App(e.into()),
                            });
                        },
                    }
                }
            };
        }
    }
}

pub struct ConcurrentRequestHandler<
    Root: crate::RpcMessage,
    Handler: crate::RootHandler<Root>,
    ParallelRoot: crate::RpcMessage,
    ParallelHandler: crate::RootHandler<ParallelRoot>,
> {
    handler: ParallelHandler,
    _marker: PhantomData<(Handler, ParallelRoot, Root)>,
}

impl<
    Root: crate::RpcMessage,
    Handler: crate::RootHandler<Root>,
    ParallelRoot: crate::RpcMessage,
    ParallelHandler: crate::RootHandler<ParallelRoot> + Clone,
> Clone for ConcurrentRequestHandler<Root, Handler, ParallelRoot, ParallelHandler>
{
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            _marker: self._marker.clone(),
        }
    }
}

impl<
    Root: crate::RpcMessage,
    Handler: crate::RootHandler<Root>,
    ParallelRoot: crate::RpcMessage,
    ParallelHandler: crate::RootHandler<ParallelRoot>,
> std::fmt::Debug for ConcurrentRequestHandler<Root, Handler, ParallelRoot, ParallelHandler>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConcurrentRequestHandler").finish()
    }
}

impl<
    Root: crate::RpcMessage,
    Handler: crate::RootHandler<Root>,
    ParallelRoot: crate::RpcMessage,
    ParallelHandler: crate::RootHandler<ParallelRoot>,
> ConcurrentRequestHandler<Root, Handler, ParallelRoot, ParallelHandler>
{
    pub fn new(handler: ParallelHandler) -> Self {
        Self {
            handler,
            _marker: PhantomData,
        }
    }
}

// impl<Handler, Root, ParallelRoot, ParallelHandler> crate::Method
//     for ConcurrentRequestHandler<Root, Handler, ParallelRoot, ParallelHandler>
// where
//     Root: crate::RpcMessage + Send,
//     Handler: crate::RootHandler<Root>,
//     ParallelHandler: crate::Method,
// {
//     type Req = Root;

//     type Res = Result<ParallelHandler::Res, Root>;

//     type Error = ParallelHandler::Error;
// }

// impl<Handler, Root, ParallelHandler> crate::Call
//     for ConcurrentRequestHandler<Root, Handler, ParallelHandler>
// where
//     Root: crate::RpcMessage
//         + Send
//         + From<<<ParallelHandler as crate::Method>::Req as TryFrom<Root>>::Error>,
//     Handler: crate::RootHandler<Root>,
//     ParallelHandler: crate::Call + std::marker::Send,
//     ParallelHandler::Req: TryFrom<Root>,
// {
//     async fn call(&mut self, value: Self::Req) -> Result<Self::Res, Self::Error> {
//         let parallel_req = match ParallelHandler::Req::try_from(value) {
//             Ok(v) => v,
//             Err(root) => return Ok(Err(root.into())),
//         };

//         Ok(Ok(self.handler.call(parallel_req).await?))
//     }
// }
//
//
impl<
    Root: crate::RpcMessage,
    Handler: crate::RootHandler<Root>,
    ParallelRoot: crate::RpcMessage,
    ParallelHandler: crate::RootHandler<ParallelRoot>,
> crate::RootHandler<Root>
    for ConcurrentRequestHandler<Root, Handler, ParallelRoot, ParallelHandler>
where
    Root:
        crate::RpcMessage + Send + std::marker::Sync + From<<ParallelRoot as TryFrom<Root>>::Error>,
    Handler: crate::RootHandler<Root>,
    ParallelHandler: crate::RootHandler<ParallelRoot> + std::marker::Send,
    ParallelRoot: TryFrom<Root> + crate::RpcMessage + std::marker::Send,
    <ParallelHandler as crate::RootHandler<ParallelRoot>>::Error: std::marker::Send,
    <ParallelHandler as crate::RootHandler<ParallelRoot>>::Response: Sync,
{
    type Response = ParallelHandler::Response;

    type Error = ConcurrentRequestHandlerError<
        ClientError<ParallelHandler::Error, ParallelHandler::Error>,
        Root,
    >;

    async fn handle<T: futures_io::AsyncWrite + Unpin + Sync + Send, TransportError: Send>(
        &mut self,
        root: Root,
        replier: crate::Replier<'_, T>,
    ) -> Result<crate::ReplyReceipt<Self::Response>, ClientError<TransportError, Self::Error>> {
        let parallel_req = match ParallelRoot::try_from(root) {
            Ok(v) => v,
            Err(root) => {
                return Err(ClientError::App(ConcurrentRequestHandlerError::Root(
                    root.into(),
                )));
            }
        };
        self.handler.handle(parallel_req, replier).await
    }
}
enum ConcurrentRequestHandlerError<ParallelError, Root> {
    ParallelHandler(ParallelError),
    Root(Root),
}

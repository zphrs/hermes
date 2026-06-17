mod concurrent_request_handler;

use std::{convert::Infallible, io::ErrorKind, pin::Pin, task::Poll};

use crate::{
    RpcMessage,
    machine_cursor::{self, SplitReceipt},
    transport::ReplyHelper,
};

use futures::{FutureExt as _, StreamExt as _, select, stream::FuturesUnordered};
use maxlen::MaxLen;
use tracing::trace;

use crate::{
    HandleOneRequestError, Method,
    machine_cursor::state_handler::concurrent_request_handler::{
        ConcurrentRequestHandler, ConcurrentRequestHandlerError,
    },
    traits::{
        self,
        method::{self, Loopback},
        state::{self, ToHandle},
    },
};

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct StateHandler<State, Role, RootMethod, Client, H>
where
    State: traits::State,
    Role: state::Role,
    Client: crate::transport::Client,
    RootMethod: crate::Method,
    H: traits::Handler<RootMethod>,
{
    _state: traits::state::Wrapper<State>,
    handler: H,
    _root_method: method::Wrapper<RootMethod>,
    role: Role,
    client: Client,
}

#[derive(Debug, thiserror::Error)]
pub enum TransitionRequestError<ClientError, HandlerError> {
    #[error("Inner client error: {0}")]
    Client(ClientError),
    #[error("from request handler: {0}")]
    Handler(#[from] traits::HandlerError<Infallible, HandlerError>),
    #[error("handle one request error: {0}")]
    HandleOneRequest(#[from] HandleOneRequestError<minicbor_io::Error, HandlerError>),
}

#[derive(Debug, thiserror::Error)]
pub enum MultipleRequestsError<ClientError, LoopbackHandlerError, RootHandlerError> {
    #[error("client sent multiple requests that could transition at the same time")]
    MultipleActivePotentialTransitions(),
    #[error("Inner client error: {0}")]
    Client(ClientError),
    #[error("handle one request error: {0}")]
    HandleOneRequest(#[from] HandleOneRequestError<minicbor_io::Error, LoopbackHandlerError>),
    #[error("handle base request error: {0}")]
    Handler(#[from] traits::HandlerError<Infallible, RootHandlerError>),
}

pub struct DelayedReceipt<Sender, Method: crate::Method> {
    sender: Sender,
    to_send: bytes::Bytes,
    _marker: method::Wrapper<Method>,
    res: Method::Res,
}

impl<Sender, Method: crate::Method> DelayedReceipt<Sender, Method> {
    pub(crate) fn map<NewMethod: crate::Method>(
        self,
        mapper: impl FnOnce(Method::Res) -> NewMethod::Res,
    ) -> DelayedReceipt<Sender, NewMethod> {
        DelayedReceipt {
            sender: self.sender,
            to_send: self.to_send,
            res: mapper(self.res),
            _marker: method::Wrapper::new(),
        }
    }
}

impl<Sender, Method: crate::Method> DelayedReceipt<Sender, Method>
where
    Method::Res: RpcMessage,
{
    pub(crate) fn new(sender: Sender, res: Method::Res) -> Self {
        let mut bytes_mut = Vec::with_capacity(<Method::Res as MaxLen>::max_len());
        let mut writer = minicbor_io::Writer::new(&mut bytes_mut);
        writer.set_max_len(<Method::Res as MaxLen>::max_len() as u32);
        writer.write(&res).expect("write should succeedH");
        Self {
            sender,
            to_send: bytes_mut.into(),
            res,
            _marker: method::Wrapper::new(),
        }
    }
}

pub struct DelayedReplier<Sender, Method: crate::Method> {
    sender: Sender,
    _marker: method::Wrapper<Method>,
}

impl<Sender, Method: crate::Method> DelayedReplier<Sender, Method> {
    pub fn new(sender: Sender) -> Self {
        Self {
            sender,
            _marker: method::Wrapper::new(),
        }
    }

    fn change_method<NewMethod: crate::Method>(
        self,
        _req: &NewMethod::Req,
    ) -> DelayedReplier<Sender, NewMethod> {
        DelayedReplier::new(self.sender)
    }
}

pub struct FinalizeFuture<Sender: futures::AsyncWrite + Unpin> {
    sender: Sender,
    to_send: bytes::Bytes,
}

impl<Sender: futures::AsyncWrite + Unpin> Future for FinalizeFuture<Sender> {
    type Output = std::io::Result<()>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = &mut *self;
        while !this.to_send.is_empty() {
            let n = core::task::ready!(Pin::new(&mut this.sender).poll_write(cx, &this.to_send))?;

            let _other = this.to_send.split_to(n);

            if n == 0 {
                return Poll::Ready(Err(std::io::ErrorKind::WriteZero.into()));
            }
        }

        Poll::Ready(Ok(()))
    }
}

impl<Sender: futures::AsyncWrite + Unpin, Method: crate::Method> DelayedReceipt<Sender, Method> {
    pub fn finalize(self) -> (Method::Res, FinalizeFuture<Sender>) {
        let Self {
            sender,
            to_send,
            res,
            ..
        } = self;
        (res, FinalizeFuture { sender, to_send })
    }
}

impl<Method: crate::Method, Sender> ReplyHelper<Method> for DelayedReplier<Sender, Method> {
    type Error = Infallible;
    type Receipt<M: crate::Method> = DelayedReceipt<Sender, M>;

    async fn reply<Error>(
        self,
        res: Method::Res,
    ) -> Result<Self::Receipt<Method>, crate::traits::HandlerError<Self::Error, Error>>
    where
        Method::Res: crate::RpcMessage,
    {
        let Self { sender, .. } = self;
        Ok(DelayedReceipt::new(sender, res))
    }

    async fn reply_with<NewMethod: crate::Method, Handler: traits::Handler<NewMethod>>(
        self,
        handler: &mut Handler,
        req: NewMethod::Req,
        convert: impl FnOnce(NewMethod::Res) -> Method::Res,
    ) -> Result<Self::Receipt<Method>, crate::traits::HandlerError<Self::Error, Handler::Error>>
    {
        Ok(handler
            .handle(self.change_method(&req), req)
            .await?
            .map(convert))
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct PendingTransitionReceipt<
    'a,
    Sender,
    State: traits::State,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Client: crate::transport::Client,
>(
    DelayedReceipt<Sender, OldMethod>,
    Role,
    &'a mut Client,
    traits::state::Wrapper<State>,
);

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<
    'a,
    Sender: Unpin + futures::AsyncWrite,
    State: traits::State,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Connection: crate::transport::Connection + PartialEq + std::fmt::Debug,
> PendingTransitionReceipt<'a, Sender, State, OldMethod, Role, Connection>
{
    /// requires passing in the [`Sender`] linked to the [`StateHandler`] that
    /// returned the [`DelayedReceipt`] to ensure that a transition is only
    /// approved after all pending requests for the previous state was
    /// completed. Sends off the transition confirmation. Use
    /// [`MachineCursor::from_split_receipt`] to construct a new cursor using
    /// the returned [`SplitReceipt`].
    pub async fn split(
        self,
        sender: machine_cursor::sender::Sender<Role, State::ClientMethod, Connection>,
    ) -> Result<(OldMethod::Res, SplitReceipt<Connection, Role>), std::io::Error>
    where
        Connection::SendStream: futures::AsyncWrite + Unpin + 'static,
        State::ClientMethod: Method,
        State::ServerMethod: Method + 'static,
    {
        let (delayed_receipt, _role, handler_conn) = self.into_parts();
        let (role, sender_conn) = sender.into_parts();
        assert_eq!(&*handler_conn, &sender_conn);
        let (res, actually_send) = delayed_receipt.finalize();
        actually_send.await?;
        Ok((res, SplitReceipt(sender_conn, role)))
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<
    'a,
    Sender,
    State: traits::State,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Client: crate::transport::Client,
> PendingTransitionReceipt<'a, Sender, State, OldMethod, Role, Client>
{
    pub(crate) fn into_parts(self) -> (DelayedReceipt<Sender, OldMethod>, Role, &'a mut Client) {
        (self.0, self.1, self.2)
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<State, Role, RootMethod, Client, H> StateHandler<State, Role, RootMethod, Client, H>
where
    State: traits::State,
    Role: state::Role,
    Client: crate::transport::Client,
    RootMethod: crate::Method,
    H: traits::Handler<RootMethod>,
{
    pub fn new<Wrapper: ToHandle<RootMethod, Role, H> + Into<traits::state::Wrapper<State>>>(
        state_wrapper: Wrapper,
        role: Role,
        handler: H,
        conn: Client,
    ) -> Self {
        let (root_method, handler) = state_wrapper.to_handle(&role, handler).into_parts();
        Self {
            _state: state_wrapper.into(),
            handler,
            _root_method: root_method,
            role: role,
            client: conn,
        }
    }

    pub async fn handle_transition_request(
        &mut self,
    ) -> Result<
        PendingTransitionReceipt<'_, Client::SendStream, State, RootMethod, Role, Client>,
        TransitionRequestError<Client::Error, H::Error>,
    >
    where
        RootMethod::Req: crate::RpcMessage,
    {
        let Self {
            _state,
            handler,
            _root_method,
            role,
            client,
        } = self;

        let stream = client
            .accept_stream()
            .await
            .map_err(TransitionRequestError::Client)?;

        let (write, read) = stream;
        let replier = DelayedReplier::new(write);
        let mut receiver = minicbor_io::AsyncReader::new(read);

        receiver.set_max_len(RootMethod::Req::max_len() as u32);

        let Some(root) = (match receiver.read::<RootMethod::Req>().await {
            Ok(v) => v,
            Err(e) => Err(HandleOneRequestError::Replier(e))?,
        }) else {
            Err(HandleOneRequestError::Replier(minicbor_io::Error::Io(
                ErrorKind::ConnectionAborted.into(),
            )))?
        };
        let out = match handler.handle(replier, root).await {
            Ok(v) => v,
            Err(e) => return Err(e)?,
        };

        return Ok(PendingTransitionReceipt(
            out,
            *role,
            client,
            state::Wrapper::new(),
        ));
    }

    /// handles requests in parallel until a method called is not a loopback.
    pub async fn handle_requests<LoopbackMethod: Loopback, LoopbackHandler>(
        &'_ mut self,
        loopback_handler: LoopbackHandler,
    ) -> Result<
        PendingTransitionReceipt<'_, Client::SendStream, State, RootMethod, Role, Client>,
        MultipleRequestsError<Client::Error, LoopbackHandler::Error, H::Error>,
    >
    where
        RootMethod::Req: crate::RpcMessage,
        LoopbackHandler: traits::Handler<LoopbackMethod> + Clone,
        LoopbackMethod::Req: TryFrom<RootMethod::Req, Error = RootMethod::Req>,
        <LoopbackMethod as Method>::Req: crate::RpcMessage,
        H: crate::traits::Handler<RootMethod>,
        <RootMethod as Method>::Res: From<<LoopbackMethod as Method>::Res>,
    {
        let Self {
            _state,
            handler,
            _root_method,
            role,
            client,
        } = self;

        let mut js = FuturesUnordered::new();
        let (root, stream) = loop {
            select! {
                maybe_stream = client.accept_stream().fuse() => {
                    let mut stream = maybe_stream.map_err(MultipleRequestsError::Client)?;
                    let handler = loopback_handler.clone();
                    js.push(async {
                    let mut concurrent_handler =
                        ConcurrentRequestHandler::<RootMethod, LoopbackMethod, LoopbackHandler>::new(
                            handler,
                        );
                        (client.handle_one_request(&mut stream, &mut concurrent_handler).await, stream)
                    });
                }
                next_result = js.select_next_some() => {
                    match next_result {
                        (Ok(_response), _) => {
                            trace!("successfully replied")
                        },
                        (Err(HandleOneRequestError::App(ConcurrentRequestHandlerError::FailedConversion(root))), stream) => {
                            // got to non-concurrent value, break out of
                            // concurrent loop so we stop handling new
                            // requests

                            break (root, stream);
                        }
                        (Err(HandleOneRequestError::App(ConcurrentRequestHandlerError::ParallelHandler(e))), _) => Err(HandleOneRequestError::App(e))?,
                        (Err(HandleOneRequestError::Replier(replier)), _) => Err(HandleOneRequestError::Replier(replier))?,
                    };
                }
            };
        };

        // wait for all concurrent requests to finish
        // executing before continuing with transition
        while let Some(next_result) = js.next().await {
            match next_result {
                (Ok(_response), _) => {
                    trace!("successfully replied")
                }
                (
                    Err(HandleOneRequestError::App(
                        ConcurrentRequestHandlerError::FailedConversion(_root),
                    )),
                    _stream,
                ) => {
                    // got transition request in middle of transition,
                    // exiting early with an error. Likely to close connection
                    // fully, assuming the caller drops the Conn after any
                    // error.
                    return Err(MultipleRequestsError::MultipleActivePotentialTransitions());
                }
                (
                    Err(HandleOneRequestError::App(
                        ConcurrentRequestHandlerError::ParallelHandler(e),
                    )),
                    _,
                ) => Err(HandleOneRequestError::App(e))?,
                (Err(HandleOneRequestError::Replier(replier)), _) => {
                    Err(HandleOneRequestError::Replier(replier))?
                }
            };
        }
        debug_assert!(js.is_empty());
        drop(js);
        let replier = DelayedReplier::new(stream.0);
        return Ok(PendingTransitionReceipt(
            handler.handle(replier, root).await?,
            role.clone(),
            client,
            state::Wrapper::new(),
        ));
    }
}

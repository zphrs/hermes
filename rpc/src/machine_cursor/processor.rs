use crate::{Handler, ImmediateReplier};
mod concurrent_request_handler;

use std::{
    convert::Infallible,
    io::ErrorKind,
    marker::PhantomData,
    pin::{Pin, pin},
    task::Poll,
};

use crate::{
    RpcMessage,
    machine_cursor::{self, SplitReceipt},
    traits::{Prioritized, state::role},
    transport::ReplyHelper,
};

use futures::{Future, FutureExt as _, StreamExt as _, select, stream::FuturesUnordered};
use maxlen::MaxLen;
use tracing::trace;

use crate::{
    HandleOneRequestError, Method,
    machine_cursor::processor::concurrent_request_handler::{
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
pub struct Processor<State, Role, RootMethod, Client, H>
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
pub enum TransitionRequestError<ClientError, HandlerError, ReplierError> {
    #[error("Inner client error: {0}")]
    Client(ClientError),
    #[error("from request handler: {0}")]
    Handler(#[from] traits::HandleError<Infallible, HandlerError>),
    #[error("handle one request error: {0}")]
    HandleOneRequest(#[from] HandleOneRequestError<ReplierError, HandlerError>),
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
    Handler(#[from] traits::HandleError<Infallible, RootHandlerError>),
}

pub struct DelayedReceipt<Method: crate::Method> {
    to_send: bytes::Bytes,
    _marker: method::Wrapper<Method>,
    res: Method::Res,
}

impl<Method: crate::Method> DelayedReceipt<Method> {
    pub(crate) fn map<NewMethod: crate::Method>(
        self,
        mapper: impl FnOnce(Method::Res) -> NewMethod::Res,
    ) -> DelayedReceipt<NewMethod> {
        DelayedReceipt {
            to_send: self.to_send,
            res: mapper(self.res),
            _marker: method::Wrapper::new(),
        }
    }
}

impl<Method: crate::Method> DelayedReceipt<Method>
where
    Method::Res: RpcMessage,
{
    pub(crate) fn new(res: Method::Res) -> Self {
        let mut bytes_mut = Vec::with_capacity(<Method::Res as MaxLen>::max_len());
        let mut writer = minicbor_io::Writer::new(&mut bytes_mut);
        writer.set_max_len(<Method::Res as MaxLen>::max_len() as u32);
        writer.write(&res).expect("write should succeedH");
        Self {
            to_send: bytes_mut.into(),
            res,
            _marker: method::Wrapper::new(),
        }
    }
}

pub struct DelayedReplier<Method: crate::Method> {
    _marker: method::Wrapper<Method>,
}

impl<Method: crate::Method, Sender> From<&mut Sender> for DelayedReplier<Method> {
    fn from(_v: &mut Sender) -> Self {
        Self::new()
    }
}

impl<Method: crate::Method> DelayedReplier<Method> {
    pub fn new() -> Self {
        Self {
            _marker: method::Wrapper::new(),
        }
    }

    fn change_method<NewMethod: crate::Method>(
        self,
        _req: &NewMethod::Req,
    ) -> DelayedReplier<NewMethod> {
        DelayedReplier::new()
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

impl<Method: crate::Method> DelayedReceipt<Method> {
    pub fn finalize<Sender: futures::AsyncWrite + Unpin>(
        self,
        sender: Sender,
    ) -> (Method::Res, FinalizeFuture<Sender>) {
        let Self { to_send, res, .. } = self;
        (res, FinalizeFuture { sender, to_send })
    }
}

impl<Method: crate::Method> ReplyHelper<Method> for DelayedReplier<Method> {
    type Error = Infallible;
    type Receipt<M: crate::Method> = DelayedReceipt<M>;

    async fn reply<Error>(
        self,
        res: Method::Res,
    ) -> Result<Self::Receipt<Method>, crate::traits::HandleError<Self::Error, Error>>
    where
        Method::Res: crate::RpcMessage,
    {
        Ok(DelayedReceipt::new(res))
    }

    async fn reply_with<NewMethod: crate::Method, Handler: traits::Handler<NewMethod>>(
        self,
        handler: &mut Handler,
        req: NewMethod::Req,
        convert: impl FnOnce(NewMethod::Res) -> Method::Res,
    ) -> Result<Self::Receipt<Method>, crate::traits::HandleError<Self::Error, Handler::Error>>
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
    State: traits::State,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Client: crate::transport::Client,
    Priority,
    Sender: futures::AsyncWrite + Unpin,
>(
    DelayedReceipt<OldMethod>,
    Role,
    &'a Client,
    traits::state::Wrapper<State>,
    Priority,
    Sender,
);

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<
    'a,
    State: traits::State,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Connection: crate::transport::Connection + PartialEq + std::fmt::Debug,
    Priority,
    Sender: futures::AsyncWrite + Unpin,
> PendingTransitionReceipt<'a, State, OldMethod, Role, Connection, Priority, Sender>
{
    /// requires passing in the [`Sender`] linked to the [`StateHandler`] that
    /// returned the [`DelayedReceipt`] to ensure that a transition is only
    /// approved after all pending requests for the previous state was
    /// completed. Sends off the transition confirmation. Use
    /// [`MachineCursor::from_split_receipt`] to construct a new cursor using
    /// the returned [`SplitReceipt`].
    pub async fn split(
        self,
        sender: machine_cursor::requester::Requester<Role, State::ClientMethod, Connection>,
    ) -> Result<(OldMethod::Res, SplitReceipt<Connection, Role>), std::io::Error>
    where
        Connection::SendStream: futures::AsyncWrite + Unpin,
        State::ClientMethod: Method,
        State::ServerMethod: Method,
    {
        let (delayed_receipt, _role, handler_conn, send_stream) = self.into_parts();
        let (role, sender_conn) = sender.into_parts();
        assert_eq!(&*handler_conn, &sender_conn);
        let (res, actually_send) = delayed_receipt.finalize(send_stream);
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
    State: traits::State,
    OldMethod: traits::Method,
    Role: traits::state::Role,
    Client: crate::transport::Client,
    Priority,
    Sender: futures::AsyncWrite + Unpin,
> PendingTransitionReceipt<'a, State, OldMethod, Role, Client, Priority, Sender>
{
    pub(crate) fn into_parts(self) -> (DelayedReceipt<OldMethod>, Role, &'a Client, Sender) {
        (self.0, self.1, self.2, self.5)
    }
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<State, Role, RootMethod, Client, H> Processor<State, Role, RootMethod, Client, H>
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

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub async fn handle_transition_request<'a>(
        &'a mut self,
    ) -> Result<
        PendingTransitionReceipt<
            'a,
            State,
            RootMethod,
            Role,
            Client,
            <State as Prioritized>::Priority,
            Client::SendStream,
        >,
        TransitionRequestError<
            Client::Error,
            H::Error,
            <DelayedReplier<RootMethod> as ReplyHelper<RootMethod>>::Error,
        >,
    >
    where
        RootMethod::Req: crate::RpcMessage + Clone,
        State: Prioritized,
    {
        let Self {
            handler,
            role,
            client,
            ..
        } = self;
        let fut = client.accept_stream();

        let mut stream = match fut.await {
            Ok(stream) => stream,
            Err(e) => Err(TransitionRequestError::Client(e))?,
        };

        let mut with_priority = WithPriority::<RootMethod, H, State, Role>::new(handler, *role);
        let receipt = {
            let replier = DelayedReplier::<RootMethod>::new();
            // inlined `client.handle_one_request_with_handler(replier, stream, handler)`
            // in order to avoid https://github.com/rust-lang/rust/issues/100013
            let stream = &mut stream.1;
            let handler = &mut with_priority;
            async move {
                let write = replier;
                let read = stream;
                let mut receiver = minicbor_io::AsyncReader::new(read);
                receiver.set_max_len(RootMethod::Req::max_len() as u32);
                let Some(root) = (match receiver.read::<RootMethod::Req>().await {
                    Ok(v) => v,
                    Err(e) => Err(HandleOneRequestError::Read(e))?,
                }) else {
                    return Err(HandleOneRequestError::Read(minicbor_io::Error::Io(
                        ErrorKind::ConnectionAborted.into(),
                    )));
                };
                let out = match handler.handle(write, root).await {
                    Ok(v) => v,
                    Err(traits::HandleError::Handler(e)) => {
                        return Err(HandleOneRequestError::App(e));
                    }
                    Err(traits::HandleError::Replier(e)) => {
                        return Err(HandleOneRequestError::Replier(e));
                    }
                };
                Ok(out)
            }
        }
        .await?;

        let priority = with_priority
            .into_priority()
            .expect("priority is set during handle");

        Ok(PendingTransitionReceipt(
            receipt,
            self.role,
            &self.client,
            state::Wrapper::new(),
            priority,
            stream.0,
        ))
    }

    /// handles requests in parallel until a method called is not a loopback.
    pub async fn handle_requests<LoopbackMethod: Loopback, LoopbackHandler>(
        &'_ mut self,
        loopback_handler: LoopbackHandler,
    ) -> Result<
        PendingTransitionReceipt<
            '_,
            State,
            RootMethod,
            Role,
            Client,
            State::Priority,
            Client::SendStream,
        >,
        MultipleRequestsError<Client::Error, LoopbackHandler::Error, H::Error>,
    >
    where
        RootMethod::Req: crate::RpcMessage,
        LoopbackHandler: traits::Handler<LoopbackMethod> + Clone,
        LoopbackMethod::Req: TryFrom<RootMethod::Req, Error = RootMethod::Req>,
        <LoopbackMethod as Method>::Req: crate::RpcMessage,
        H: crate::traits::Handler<RootMethod>,
        <RootMethod as Method>::Res: From<<LoopbackMethod as Method>::Res>,
        State: Prioritized,
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
                    // inlined `client.handle_one_request(stream, concurrent_handler)`
                    // in order to avoid https://github.com/rust-lang/rust/issues/100013
                    let out = {
                        let this = &client;
                        let stream = &mut stream;
                        let handler = &mut concurrent_handler;
                        let (write, read) = stream;
                        let replier = ImmediateReplier::from(write);
                        let out = {
                            async move {
                                let write = replier;
                                let read = read;
                                let mut receiver = minicbor_io::AsyncReader::new(read);
                                receiver.set_max_len(RootMethod::Req::max_len() as u32);
                                let Some(root) = (match receiver.read::<RootMethod::Req>().await {
                                    Ok(v) => v,
                                    Err(e) => Err(HandleOneRequestError::Read(e))?,
                                }) else {
                                    return Err(HandleOneRequestError::Read(minicbor_io::Error::Io(
                                        ErrorKind::ConnectionAborted.into(),
                                    )));
                                };
                                let out = match handler.handle(write, root).await {
                                    Ok(v) => v,
                                    Err(traits::HandleError::Handler(e)) => {
                                        return Err(HandleOneRequestError::App(e));
                                    }
                                    Err(traits::HandleError::Replier(e)) => {
                                        return Err(HandleOneRequestError::Replier(e));
                                    }
                                };
                                Ok(out)
                            }
                        }
                            .map(|v| v.map(|v| v.into_inner()));
                        out
                    }.await;

                        (out, stream)
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
                        (Err(HandleOneRequestError::Read(replier)), _) => Err(HandleOneRequestError::Read(replier))?,
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
                    _,
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
                (Err(HandleOneRequestError::Read(replier)), _) => {
                    Err(HandleOneRequestError::Read(replier))?
                }
            };
        }
        debug_assert!(js.is_empty());
        drop(js);
        // SAFETY: RootMethod aligns with the type
        // `State::ClientMethod::Req` if the Role is indeed Client.
        // It's just not possible to validate such without duplicating
        // this function with two near-identical functions, just with
        // the following unsafe block replaced with State::client_priority(&root)
        // if the Role is role::Client and State::server_priority(&root) if the
        // Role is role::Server.
        //
        // Since Role cannot be implemented outside of this crate due to sealing,
        // it's perfectly safe to do so.
        let priority = unsafe {
            match Role::to_enum() {
                role::WhichRole::Client => State::client_priority(&std::mem::transmute::<
                    &RootMethod::Req,
                    &<State::ClientMethod as crate::Method>::Req,
                >(&root)),
                role::WhichRole::Server => State::server_priority(&std::mem::transmute::<
                    &RootMethod::Req,
                    &<State::ServerMethod as crate::Method>::Req,
                >(&root)),
            }
        };
        let replier = DelayedReplier::new();
        return Ok(PendingTransitionReceipt(
            handler.handle(replier, root).await?,
            role.clone(),
            client,
            state::Wrapper::new(),
            priority,
            stream.0,
        ));
    }
}

/// Will always error out with a HandlerError::Handler(T::Req)
/// purely to extract the sent value and handle it differently.
///
/// Used for handle_transition_request
struct WithPriority<
    'a,
    T: Method,
    Handler: crate::Handler<T>,
    State: Prioritized,
    Role: traits::state::Role,
> {
    method: method::Wrapper<T>,
    handler: &'a mut Handler,
    state: state::Wrapper<State>,
    role: Role,
    priority: Option<State::Priority>,
}

impl<
    'a,
    M: crate::Method,
    Handler: crate::Handler<M>,
    State: Prioritized,
    Role: traits::state::Role,
> WithPriority<'a, M, Handler, State, Role>
{
    fn new(handler: &'a mut Handler, role: Role) -> Self {
        Self {
            method: method::Wrapper::new(),
            handler,
            state: state::Wrapper::new(),
            role,
            priority: None,
        }
    }

    pub fn into_priority(self) -> Option<State::Priority> {
        self.priority
    }
}

impl<
    'a,
    T: crate::Method,
    Handler: crate::Handler<T>,
    State: Prioritized,
    Role: traits::state::Role,
> crate::Handler<T> for WithPriority<'a, T, Handler, State, Role>
{
    type Error = Handler::Error;

    async fn handle<Replier: ReplyHelper<T>>(
        &mut self,
        replier: Replier,
        value: <T as Method>::Req,
    ) -> Result<
        <Replier as ReplyHelper<T>>::Receipt<T>,
        traits::HandleError<<Replier as ReplyHelper<T>>::Error, <Self as crate::Handler<T>>::Error>,
    > {
        // SAFETY: RootMethod aligns with the type
        // `State::ClientMethod::Req` if the Role is indeed Client.
        // It's just not possible to validate such without duplicating
        // this function with two near-identical functions, just with
        // the following unsafe block replaced with State::client_priority(&root)
        // if the Role is role::Client and State::server_priority(&root) if the
        // Role is role::Server.
        //
        // Since Role cannot be implemented outside of this crate due to sealing,
        // it's perfectly safe to assume the two types are identical.
        let priority = unsafe {
            match Role::to_enum() {
                role::WhichRole::Client => State::client_priority(&std::mem::transmute::<
                    &<T as crate::Method>::Req,
                    &<State::ClientMethod as crate::Method>::Req,
                >(&value)),
                role::WhichRole::Server => State::server_priority(&std::mem::transmute::<
                    &<T as crate::Method>::Req,
                    &<State::ServerMethod as crate::Method>::Req,
                >(&value)),
            }
        };

        self.priority = Some(priority);

        self.handler.handle(replier, value).await
    }
}

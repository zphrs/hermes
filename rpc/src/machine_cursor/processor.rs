use crate::machine_cursor::transition::processor::ProcessorTransition;
use crate::method::{Ancestor, FromDescendant, is_leaf};
use crate::{Handler, ImmediateReplier, machine_cursor::PendingTransitionReceipt};
mod concurrent_request_handler;

use std::pin::Pin;
use std::{convert::Infallible, io::ErrorKind};

use crate::state::{PrioritizedUnsafeExt, Wrapper};

use crate::{traits::Prioritized, transport::ReplyHelper};

use futures::{
    FutureExt as _, StreamExt as _, future::FusedFuture, select, stream::FuturesUnordered,
};
use maxlen::MaxLen;
use tracing::{debug, trace};

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

use super::transition::processor::DelayedReplier;

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
    H: traits::Handler<RootMethod, RootMethod>,
{
    _state: traits::state::Wrapper<State>,
    handler: H,
    _root_method: method::Wrapper<RootMethod>,
    role: Role,
    client: Client,
}

#[must_use = "should be explicitly awaited or alternatively passed into a tiebreak \
    function to be awaited there"]
pub struct EventualTransitionRequest<Fut> {
    fut: Pin<Box<Fut>>,
    done: bool,
}

impl<Fut> EventualTransitionRequest<Fut> {
    pub(crate) fn new(fut: Fut) -> Self {
        Self {
            fut: Box::pin(fut),
            done: false,
        }
    }
}

impl<Fut: futures::Future> FusedFuture for EventualTransitionRequest<Fut> {
    fn is_terminated(&self) -> bool {
        self.done
    }
}

impl<Fut: Future> Future for EventualTransitionRequest<Fut> {
    type Output = Fut::Output;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let out = self.fut.as_mut().poll(cx);

        if out.is_ready() {
            self.done = true;
        }

        out
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TransitionRequestError<ClientError, HandlerError, ReplierError> {
    #[error("inner client error: {0}")]
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
    #[error("inner client error: {0}")]
    Client(ClientError),
    #[error("handle one request error: {0}")]
    HandleOneRequest(#[from] HandleOneRequestError<minicbor_io::Error, LoopbackHandlerError>),
    #[error("handle base request error: {0}")]
    Handler(#[from] traits::HandleError<minicbor::encode::Error<Infallible>, RootHandlerError>),
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
    H: traits::Handler<RootMethod, RootMethod>,
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

    pub fn handle_transition_request<'a>(
        self,
    ) -> EventualTransitionRequest<
        impl Future<
            Output = Result<
                ProcessorTransition<
                    super::transition::processor::Entrypoint<State, RootMethod, Role, Client>,
                >,
                TransitionRequestError<
                    Client::Error,
                    H::Error,
                    <DelayedReplier<RootMethod> as ReplyHelper<RootMethod, RootMethod>>::Error,
                >,
            >,
        >,
    >
    where
        RootMethod::Req: crate::RpcMessage,
        State: Prioritized,
        Client: 'a,
    {
        let fut = async move {
            let Self {
                mut handler,
                role,
                ref client,
                _state,
                ..
            } = self;
            let fut = client.accept_stream();

            let mut stream = match fut.await {
                Ok(stream) => stream,
                Err(e) => Err(TransitionRequestError::Client(e))?,
            };

            let mut with_priority =
                WithPriority::<RootMethod, H, State, Role>::new(&mut handler, role, _state);
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

            let (priority, state) = with_priority
                .into_parts()
                .expect("priority is set during handle");

            Ok(ProcessorTransition::new(PendingTransitionReceipt::new(
                receipt,
                self.role,
                self.client,
                state,
                priority,
                stream.0,
            )))
        };

        EventualTransitionRequest::new(fut)
    }

    /// handles requests in parallel until a method called is not a loopback.
    pub fn handle_requests<LoopbackMethod: Loopback, LoopbackHandler>(
        self,
        loopback_handler: LoopbackHandler,
    ) -> EventualTransitionRequest<
        impl Future<
            Output = Result<
                ProcessorTransition<
                    super::transition::processor::Entrypoint<State, RootMethod, Role, Client>,
                >,
                MultipleRequestsError<Client::Error, LoopbackHandler::Error, H::Error>,
            >,
        >,
    >
    where
        RootMethod::Req: crate::RpcMessage,
        LoopbackHandler: traits::Handler<RootMethod, LoopbackMethod> + Clone,
        <LoopbackMethod as Method>::Req: crate::RpcMessage,
        H: crate::traits::Handler<RootMethod, RootMethod>,
        State: Prioritized,
        RootMethod: Ancestor<LoopbackMethod>,
        RootMethod: FromDescendant<LoopbackMethod, IsLeaf = is_leaf::False>,
    {
        let fut = async move {
            let Self {
                _state: state,
                mut handler,
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
                            let _this = &client;
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
                                debug!("breaking out of loop");
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
            let mut with_priority = WithPriority::new(&mut handler, role, state);
            let replier = DelayedReplier::new();
            let receipt = with_priority.handle(replier, root).await?;
            let (priority, state) = with_priority
                .into_parts()
                .expect("priority is set during handle");

            Ok(ProcessorTransition::new(PendingTransitionReceipt::new(
                receipt, self.role, client, state, priority, stream.0,
            )))
        };
        EventualTransitionRequest::new(fut)
    }
}

/// Will always error out with a HandlerError::Handler(T::Req)
/// purely to extract the sent value and handle it differently.
///
/// Used for handle_transition_request
struct WithPriority<
    'a,
    T: Method,
    Handler: crate::Handler<T, T>,
    State: Prioritized,
    Role: traits::state::Role,
> {
    _method: method::Wrapper<T>,
    handler: &'a mut Handler,
    _state: state::Wrapper<State>,
    _role: Role,
    priority: Option<State::Priority>,
}

impl<
    'a,
    M: crate::Method,
    Handler: crate::Handler<M, M>,
    State: Prioritized,
    Role: traits::state::Role,
> WithPriority<'a, M, Handler, State, Role>
{
    fn new(handler: &'a mut Handler, role: Role, state: state::Wrapper<State>) -> Self {
        Self {
            _method: method::Wrapper::new(),
            handler,
            _state: state,
            _role: role,
            priority: None,
        }
    }

    pub fn into_parts(self) -> Option<(State::Priority, Wrapper<State>)> {
        self.priority.map(|v| (v, self._state))
    }
}

impl<
    'a,
    M: crate::Method,
    Handler: crate::Handler<M, M>,
    State: Prioritized,
    Role: traits::state::Role,
> crate::Handler<M, M> for WithPriority<'a, M, Handler, State, Role>
{
    type Error = Handler::Error;

    async fn handle<Replier: ReplyHelper<M, M>>(
        &mut self,
        replier: Replier,
        value: <M as Method>::Req,
    ) -> Result<
        <Replier as ReplyHelper<M, M>>::Receipt<M>,
        traits::HandleError<
            <Replier as ReplyHelper<M, M>>::Error,
            <Self as crate::Handler<M, M>>::Error,
        >,
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
        let priority = unsafe { State::processor_priority::<Role, _>(&value) };

        self.priority = Some(priority);

        self.handler.handle(replier, value).await
    }
}

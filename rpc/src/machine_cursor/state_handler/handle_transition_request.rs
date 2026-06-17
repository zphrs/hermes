use std::{
    io::ErrorKind,
    pin::{Pin, pin},
    task::{Context, Poll, ready},
};

use futures::FutureExt as _;
use maxlen::MaxLen;

use crate::{
    HandleOneRequestError, Handler as _,
    machine_cursor::{
        TransitionRequestError,
        state_handler::{DelayedReplier, PendingTransitionReceipt, WithPriority},
    },
    state::{self, Prioritized},
    traits::{self, method},
    transport::ReplyHelper,
};

/// State machine for [`StateHandler::handle_transition_request`].
///
/// Implements [`Future`] without async blocks to avoid the compiler bug
/// described in <https://github.com/rust-lang/rust/issues/100013>.
#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
pub struct HandleTransitionRequest<'a, State, Role, RootMethod, Client, H>
where
    State: traits::State + Prioritized,
    Role: state::Role,
    Client: crate::transport::Client,
    RootMethod: crate::Method,
    H: traits::Handler<RootMethod>,
{
    pub(super) client: Option<&'a Client>,
    pub(super) role: Role,
    pub(super) inner: HandleTransitionRequestState<'a, State, Role, RootMethod, Client, H>,
}

pub(super) enum HandleTransitionRequestState<
    'a,
    State,
    Role,
    RootMethod,
    Client: crate::transport::Client,
    H,
> where
    State: traits::State + Prioritized,
    Role: state::Role,
    RootMethod: crate::Method,
    H: traits::Handler<RootMethod>,
{
    /// Waiting for the client to accept an incoming stream.
    AcceptStream {
        fut: Client::AcceptStreamFut,
        handler: &'a mut H,
    },
    /// Stream accepted; waiting to read the request off it.
    ReadRequest {
        reader: minicbor_io::AsyncReader<Client::RecvStream>,
        write: Client::SendStream,
        with_priority: WithPriority<'a, RootMethod, H, State, Role>,
    },
    /// Request read; waiting for the handler to produce a [`DelayedReceipt`].
    ///
    /// # Contract
    /// [`DelayedReplier`] never suspends, so the inner `handle` future must
    /// complete within a single poll.  Panicking here surfaces a programming
    /// error in the [`Handler`](traits::Handler) implementation.
    HandleRequest {
        with_priority: WithPriority<'a, RootMethod, H, State, Role>,
        req: RootMethod::Req,
        write: Client::SendStream,
    },
    Done,
}

impl<'a, State, Role, RootMethod, Client, H> Future
    for HandleTransitionRequest<'a, State, Role, RootMethod, Client, H>
where
    State: traits::State + Prioritized,
    Role: state::Role,
    Client: crate::transport::Client,
    RootMethod: crate::Method + Unpin,
    RootMethod::Req: crate::RpcMessage + Clone,
    State: Unpin,
    <State as Prioritized>::Priority: Unpin,
    H: traits::Handler<RootMethod>,
    <RootMethod as method::Method>::Req: Unpin,
    Role: Unpin,
{
    type Output = Result<
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
    >;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // `AcceptStreamFut: Unpin` (trait bound), and all other stored types are
        // composed of references or PhantomData, so `HandleTransitionRequest: Unpin`.
        let this = &mut *self;

        loop {
            match &mut this.inner {
                HandleTransitionRequestState::AcceptStream { fut, .. } => {
                    // `AcceptStreamFut: Unpin`, so `Pin::new` is safe.
                    let (write, read) = match ready!(Pin::new(fut).poll(cx)) {
                        Ok(stream) => stream,
                        Err(e) => return Poll::Ready(Err(TransitionRequestError::Client(e))),
                    };

                    let old =
                        std::mem::replace(&mut this.inner, HandleTransitionRequestState::Done);
                    let HandleTransitionRequestState::AcceptStream { handler, .. } = old else {
                        unreachable!()
                    };

                    let with_priority =
                        WithPriority::<RootMethod, _, State, Role>::new(handler, this.role);
                    let mut reader = minicbor_io::AsyncReader::new(read);
                    reader.set_max_len(<RootMethod::Req as MaxLen>::max_len() as u32);
                    this.inner = HandleTransitionRequestState::ReadRequest {
                        reader,
                        write,
                        with_priority,
                    };
                }

                HandleTransitionRequestState::ReadRequest { reader, .. } => {
                    // `AsyncReader` stores all buffering state internally, so it is
                    // safe to re-create the `read` future on each poll invocation.
                    let req = match ready!(pin!(reader.read::<RootMethod::Req>()).poll_unpin(cx)) {
                        Ok(Some(req)) => req,
                        Ok(None) => {
                            return Poll::Ready(Err(TransitionRequestError::HandleOneRequest(
                                HandleOneRequestError::Read(minicbor_io::Error::Io(
                                    ErrorKind::ConnectionAborted.into(),
                                )),
                            )));
                        }
                        Err(e) => {
                            return Poll::Ready(Err(TransitionRequestError::HandleOneRequest(
                                HandleOneRequestError::Read(e),
                            )));
                        }
                    };

                    let old =
                        std::mem::replace(&mut this.inner, HandleTransitionRequestState::Done);
                    let HandleTransitionRequestState::ReadRequest {
                        write,
                        with_priority,
                        ..
                    } = old
                    else {
                        unreachable!()
                    };
                    this.inner = HandleTransitionRequestState::HandleRequest {
                        with_priority,
                        req,
                        write,
                    };
                }

                HandleTransitionRequestState::HandleRequest {
                    with_priority, req, ..
                } => {
                    // `DelayedReplier` is synchronous — its `reply` impl returns
                    // `Poll::Ready` immediately and performs no I/O.  A handler
                    // that suspends here is a programming error.
                    let receipt = match pin!(
                        with_priority.handle(DelayedReplier::<RootMethod>::new(), req.clone())
                    )
                    .poll_unpin(cx)
                    {
                        Poll::Ready(Ok(receipt)) => receipt,
                        Poll::Ready(Err(e)) => {
                            return Poll::Ready(Err(TransitionRequestError::HandleOneRequest(
                                e.into(),
                            )));
                        }
                        Poll::Pending => {
                            unreachable!(
                                "handle with DelayedReplier must complete synchronously; \
                                 handlers used for state transitions must not suspend"
                            )
                        }
                    };

                    let old =
                        std::mem::replace(&mut this.inner, HandleTransitionRequestState::Done);
                    let HandleTransitionRequestState::HandleRequest {
                        with_priority,
                        write,
                        ..
                    } = old
                    else {
                        unreachable!()
                    };
                    let priority = with_priority
                        .into_priority()
                        .expect("priority is set during handle");

                    return Poll::Ready(Ok(PendingTransitionReceipt(
                        receipt,
                        this.role,
                        this.client.take().unwrap(),
                        state::Wrapper::new(),
                        priority,
                        write,
                    )));
                }

                HandleTransitionRequestState::Done => {
                    unreachable!("HandleTransitionRequest polled after completion")
                }
            }
        }
    }
}

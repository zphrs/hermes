use std::{
    marker::PhantomData,
    pin::pin,
    task::{Poll, Waker, ready},
};

use futures::{FutureExt as _, future::FusedFuture};
use maxlen::MaxLen;
use tracing::debug;

use crate::{Caller, transport::BiStream};

enum QueryState<C: Caller> {
    Entrypoint(Option<(C::SendStream, C::RecvStream)>),
    SenderFut(
        minicbor_io::AsyncWriter<<C as BiStream>::SendStream>,
        Option<<C as BiStream>::RecvStream>,
    ),
    ReceiverFut(minicbor_io::AsyncReader<<C as BiStream>::RecvStream>),
}
impl<C: Caller, M: crate::Method, RootReq> Unpin for PendingQueryOwned<C, M, RootReq> {}

// We define a custom future implementation for [`Caller::query`] to allow
/// for inspecting the RootReq and the Caller mid-resolution of the future.
pub struct PendingQueryOwned<C: Caller, M: crate::Method, RootReq> {
    caller: Option<C>,
    req: Option<RootReq>,
    state: QueryState<C>,
    _marker: PhantomData<M>,
    cancel: bool,
    waker: Option<Waker>,
}

impl<C: Caller, M: crate::Method, RootReq> PendingQueryOwned<C, M, RootReq> {
    pub fn new(caller: C, req: RootReq, stream: (C::SendStream, C::RecvStream)) -> Self {
        Self {
            caller: caller.into(),
            req: Some(req),
            state: QueryState::Entrypoint(stream.into()),
            _marker: PhantomData,
            cancel: false,
            waker: None,
        }
    }

    pub fn caller(&self) -> Option<&'_ C> {
        self.caller.as_ref()
    }
    /// Ensures we abort after we finish
    pub fn abort_early(mut self) -> Self {
        assert!(!self.cancel);
        self.cancel = true;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
        self
    }

    pub fn root_req(&self) -> Option<&RootReq> {
        self.req.as_ref()
    }
}

impl<C: Caller, M: crate::Method, RootReq: crate::RpcMessage> FusedFuture
    for PendingQueryOwned<C, M, RootReq>
where
    M::Res: crate::RpcMessage,
    RootReq: crate::RpcMessage,
{
    fn is_terminated(&self) -> bool {
        self.caller.is_none()
    }
}

pub enum Error<Transport, C, RootReq> {
    Minicbor(minicbor_io::Error),
    Transport(Transport),
    Cancelled(C, RootReq),
    Closed,
}

impl<C: Caller, M: crate::Method, RootReq> Future for PendingQueryOwned<C, M, RootReq>
where
    M::Res: crate::RpcMessage,
    RootReq: crate::RpcMessage,
{
    type Output = Result<(M::Res, C, RootReq), Error<C::Error, C, RootReq>>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let Self {
            caller,
            req,
            state,
            _marker,
            cancel,
            waker,
        } = &mut *self;
        if let Some(waker) = waker {
            waker.clone_from(cx.waker());
        } else {
            *waker = cx.waker().clone().into();
        }
        match state {
            QueryState::Entrypoint(stream) => {
                let (write, read) = stream.take().unwrap();
                let mut sender = minicbor_io::AsyncWriter::new(write);
                let _ = pin!(sender.write(&req)).poll_unpin(cx);
                let sender_future = QueryState::SenderFut(sender, Some(read));
                *state = sender_future;
            }
            QueryState::SenderFut(sender, read) => {
                let sync_fut = sender.sync();
                if let Err(e) = ready!(pin!(sync_fut).poll_unpin(cx)) {
                    return Poll::Ready(Err(Error::Minicbor(e)));
                };
                debug!("sent query");
                let mut receiver = minicbor_io::AsyncReader::new(read.take().expect(
                    "read should be defined no matter what if we're here in the state machine",
                ));

                receiver.set_max_len(<M::Res as MaxLen>::max_len() as u32);
                if *cancel {
                    return Poll::Ready(Err(Error::Cancelled(
                        caller.take().expect("shouldn't get here more than once"),
                        req.take().expect("shouldn't get here more than once"),
                    )));
                };
                *state = QueryState::ReceiverFut(receiver);
                // drops write here to indicate no more writes will occur
            }
            QueryState::ReceiverFut(receiver) => {
                if *cancel {
                    return Poll::Ready(Err(Error::Cancelled(
                        caller.take().expect("shouldn't get here more than once"),
                        req.take().expect("shouldn't get here more than once"),
                    )));
                }
                let out = match ready!(pin!(receiver.read::<M::Res>()).poll_unpin(cx)) {
                    Err(e) => Err(Error::Minicbor(e)),
                    Ok(Some(out)) => Ok(out),
                    Ok(None) => Err(Error::Closed),
                };
                debug!("received message");
                return Poll::Ready(out.map(|out| {
                    (
                        out,
                        caller.take().expect("shouldn't get here more than once"),
                        req.take().expect("shouldn't get here more than once"),
                    )
                }));
            }
        };
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

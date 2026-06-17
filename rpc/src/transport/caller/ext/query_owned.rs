use std::{
    marker::PhantomData,
    pin::pin,
    task::{Poll, ready},
};

use futures::FutureExt as _;
use maxlen::MaxLen;
use tracing::debug;

use crate::{Caller, CallerError, transport::BiStream};

enum QueryState<C: Caller> {
    Entrypoint(),
    StreamFut(C::OpenStreamFut),
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
    req: RootReq,
    state: QueryState<C>,
    _marker: PhantomData<M>,
}

impl<C: Caller, M: crate::Method, RootReq> PendingQueryOwned<C, M, RootReq>
where
    RootReq: From<M::Req>,
{
    pub fn new(caller: C, req: M::Req) -> Self {
        Self {
            caller: caller.into(),
            req: req.into(),
            state: QueryState::Entrypoint(),
            _marker: PhantomData,
        }
    }

    pub fn caller(&self) -> Option<&'_ C> {
        self.caller.as_ref()
    }
    /// aborts the request and returns the future.
    ///
    /// # Panics
    ///
    /// Panics if the caller's future has already resolved successfully.
    pub fn abort(mut self) -> C {
        self.caller
            .take()
            .expect("A PendingQuery should not be aborted if future has already resolved")
    }

    pub fn root_req(&self) -> &RootReq {
        &self.req
    }
}

impl<C: Caller, M: crate::Method, RootReq> Future for PendingQueryOwned<C, M, RootReq>
where
    M::Res: crate::RpcMessage,
    RootReq: crate::RpcMessage,
{
    type Output = Result<(M::Res, C), CallerError<C::Error>>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let Self {
            caller,
            req,
            state,
            _marker,
        } = &mut *self;
        match state {
            QueryState::Entrypoint() => {
                let stream = caller.as_ref().unwrap().open_stream();
                *state = QueryState::StreamFut(stream)
            }
            QueryState::StreamFut(stream_fut) => {
                let res = ready!(pin!(stream_fut).poll_unpin(cx));
                let (write, read) = match res {
                    Ok(v) => v,
                    Err(e) => return Poll::Ready(Err(CallerError::Transport(e))),
                };
                debug!("sending query");
                {
                    let mut sender = minicbor_io::AsyncWriter::new(write);
                    let _ = pin!(sender.write(&req)).poll_unpin(cx);
                    let sender_future = QueryState::SenderFut(sender, Some(read));
                    *state = sender_future;
                }
            }
            QueryState::SenderFut(sender, read) => {
                let sync_fut = sender.sync();
                match ready!(pin!(sync_fut).poll_unpin(cx)) {
                    Err(e) => return Poll::Ready(Err(CallerError::Minicbor(e))),
                    Ok(_) => (),
                };
                debug!("sent query");
                let mut receiver = minicbor_io::AsyncReader::new(read.take().expect(
                    "read should be defined no matter what if we're here in the state machine",
                ));

                receiver.set_max_len(<M::Res as MaxLen>::max_len() as u32);
                *state = QueryState::ReceiverFut(receiver);
                // drops write here to indicate no more writes will occur
            }
            QueryState::ReceiverFut(receiver) => {
                let out = match ready!(pin!(receiver.read::<M::Res>()).poll_unpin(cx)) {
                    Err(e) => Err(CallerError::Minicbor(e)),
                    Ok(Some(out)) => Ok(out),
                    Ok(None) => Err(CallerError::Closed),
                };
                debug!("received message");
                return Poll::Ready(out.map(|out| (out, caller.take().expect(""))));
            }
        };
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

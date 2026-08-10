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
    StreamFut(C::OpenStreamFut),
    SenderFut(
        minicbor_io::AsyncWriter<<C as BiStream>::SendStream>,
        Option<<C as BiStream>::RecvStream>,
    ),
    ReceiverFut(minicbor_io::AsyncReader<<C as BiStream>::RecvStream>),
}
impl<C: Caller, M: crate::Method, RootReq> Unpin for PendingQuery<C, M, RootReq> {}

// We define a custom future implementation for [`Caller::query`] to allow
/// for inspecting the RootReq and the Caller mid-resolution of the future.
pub struct PendingQuery<C: Caller, M: crate::Method, RootReq> {
    req: RootReq,
    state: QueryState<C>,
    _marker: PhantomData<M>,
}

impl<C: Caller, M: crate::Method, RootReq> PendingQuery<C, M, RootReq> {
    pub fn new(caller: &C, req: RootReq) -> Self {
        let stream_fut = caller.open_stream();
        Self {
            req: req.into(),
            state: QueryState::StreamFut(stream_fut),
            _marker: PhantomData,
        }
    }
}

impl<C: Caller, M: crate::Method, RootReq> Future for PendingQuery<C, M, RootReq>
where
    M::Res: crate::RpcMessage,
    RootReq: crate::RpcMessage,
{
    type Output = Result<M::Res, CallerError<C::Error>>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = &mut *self;
        loop {
            match &mut this.state {
                QueryState::StreamFut(stream_fut) => {
                    let res = ready!(pin!(stream_fut).poll_unpin(cx));
                    let (write, read) = match res {
                        Ok(v) => v,
                        Err(e) => return Poll::Ready(Err(CallerError::Transport(e))),
                    };
                    debug!("sending query");
                    {
                        let mut sender = minicbor_io::AsyncWriter::new(write);
                        let _ = pin!(sender.write(&this.req)).poll_unpin(cx);
                        let sender_future = QueryState::SenderFut(sender, Some(read));
                        this.state = sender_future;
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
                    this.state = QueryState::ReceiverFut(receiver);
                    // drops write here to indicate no more writes will occur
                }
                QueryState::ReceiverFut(receiver) => {
                    let out = match ready!(pin!(receiver.read::<M::Res>()).poll_unpin(cx)) {
                        Err(e) => Err(CallerError::Minicbor(e)),
                        Ok(Some(out)) => Ok(out),
                        Ok(None) => Err(CallerError::Closed),
                    };
                    debug!("received response");
                    return Poll::Ready(out);
                }
            };
        }
    }
}

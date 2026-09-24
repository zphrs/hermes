use std::{
    future::{pending, poll_fn},
    pin::{Pin, pin},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    task::Poll,
};

use futures::{
    future::{Either, select},
    task::AtomicWaker,
};
use tracing::debug;

use crate::io;

use super::transition;

/// Shared between a [`ProcessorFut`] and the repliers it constructs so that
/// the [`ProcessorFut`] can tell when a transition request has reached its
/// leaf handler and has been rejected.
///
/// All flags live in one atomic so that `run_leaf` and `reject` are totally
/// ordered: at least one side always observes the other's flag.
#[derive(Clone, Default)]
pub(crate) struct TransitionGate(Arc<GateInner>);

#[derive(Default)]
struct GateInner {
    flags: AtomicU8,
    /// woken when [`TransitionGate::reject`] is called so that a running leaf
    /// handler can be abandoned
    leaf: AtomicWaker,
}

impl TransitionGate {
    const ARRIVED: u8 = 1 << 0;
    const REJECTING: u8 = 1 << 1;
    const REJECT_SENT: u8 = 1 << 2;

    /// Called by repliers to run a transition leaf handler. Passes `stream`
    /// through along with the leaf's output.
    ///
    /// If the [`ProcessorFut`] is rejecting transitions before or while the
    /// leaf runs, the leaf is dropped (or never started), the request is
    /// explicitly rejected on `stream`, and this hangs forever; the
    /// [`ProcessorFut`] then drops the hanging future.
    pub(crate) async fn run_leaf<SendStream: io::BytesWriteStream, Leaf: Future>(
        &self,
        stream: SendStream,
        leaf: Leaf,
    ) -> (SendStream, Leaf::Output) {
        if self.0.flags.fetch_or(Self::ARRIVED, Ordering::Relaxed) & Self::REJECTING == 0 {
            let rejecting = poll_fn(|cx| {
                self.0.leaf.register(cx.waker());
                if self.rejecting() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            });
            if let Either::Left((out, _)) = select(pin!(leaf), pin!(rejecting)).await {
                return (stream, out);
            }
        }
        if let Err(e) = transition::delayed_replier::reject(stream).await {
            // the peer will observe a read error instead
            debug!(?e, "could not send transition rejection");
        }
        self.0.flags.fetch_or(Self::REJECT_SENT, Ordering::Relaxed);
        pending().await
    }

    fn rejecting(&self) -> bool {
        self.0.flags.load(Ordering::Relaxed) & Self::REJECTING != 0
    }

    fn rejection_sent(&self) -> bool {
        self.0.flags.load(Ordering::Relaxed) & Self::REJECT_SENT != 0
    }

    fn reject(&self) {
        self.0.flags.fetch_or(Self::REJECTING, Ordering::Relaxed);
        self.0.leaf.wake();
    }
}

/// marker struct for any future that takes ownership of a [`Processor`].
/// Dropping this future MUST drop the owned Processor as well.
#[pin_project::pin_project]
pub struct ProcessorFut<Fut> {
    #[pin]
    fut: Option<Fut>,
    gate: TransitionGate,
}

impl<Fut> ProcessorFut<Fut> {
    pub(super) fn new(with_transition_gate: impl FnOnce(TransitionGate) -> Fut) -> Self {
        let gate = TransitionGate::default();
        ProcessorFut {
            fut: Some(with_transition_gate(gate.clone())),
            gate,
        }
    }

    /// Drops the inner future (and with it any in-flight handlers and their
    /// streams). Polling afterwards is always pending.
    pub(crate) fn cancel(self: Pin<&mut Self>) {
        self.project().fut.set(None);
    }

    /// Rejects the transition request the peer has sent (or is about to send)
    /// to this processor without running its leaf handler to completion.
    ///
    /// Keeps polling the processor until the transition request reaches its
    /// leaf and an explicit rejection has been sent on its
    /// [`SendStream`](io::Connection::SendStream), then drops the processor.
    /// If the leaf handler already finished, its held reply is discarded and
    /// the rejection is sent in its place.
    pub(crate) async fn reject_transition<State, Role, C, Res, NextHandler, E>(
        mut self: Pin<&mut Self>,
    ) -> Result<(), E>
    where
        C: io::Connection,
        Fut: Future<Output = Result<transition::Entrypoint<State, Role, C, Res, NextHandler>, E>>,
    {
        self.gate.reject();
        let replied = poll_fn(|cx| {
            let mut this = self.as_mut().project();
            if this.gate.rejection_sent() {
                return Poll::Ready(Ok(None));
            }
            let Some(fut) = this.fut.as_mut().as_pin_mut() else {
                return Poll::Ready(Ok(None));
            };
            match fut.poll(cx) {
                Poll::Ready(res) => Poll::Ready(res.map(Some)),
                Poll::Pending if this.gate.rejection_sent() => Poll::Ready(Ok(None)),
                Poll::Pending => Poll::Pending,
            }
        })
        .await;
        self.as_mut().cancel();
        if let Some(processor_transition) = replied?
            && let Err(e) = processor_transition.reject().await
        {
            // the peer will observe a read error instead
            debug!(?e, "could not send transition rejection");
        }
        Ok(())
    }
}

impl<Fut, Output> Future for ProcessorFut<Fut>
where
    Fut: Future<Output = Output>,
{
    type Output = Output;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        match self.project().fut.as_pin_mut() {
            Some(fut) => fut.poll(cx),
            None => Poll::Pending,
        }
    }
}

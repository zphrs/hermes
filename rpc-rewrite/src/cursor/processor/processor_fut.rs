use std::{
    convert::Infallible,
    future::{Pending, pending},
};

use crate::{
    io,
    markers::NotApplicable,
    traits::{self},
};

use super::{
    Processor,
    transitions::processor_transition::{ProcessorTransition, ReplyPrimed},
};

/// marker struct for any future that takes ownership of a [`Processor`].
/// Dropping this future MUST drop the owned Processor as well.
#[pin_project::pin_project]
pub struct ProcessorFut<Fut> {
    #[pin]
    fut: Fut,
    cancelled: bool,
}

impl<Fut> ProcessorFut<Fut> {
    pub(super) fn new(fut: Fut) -> Self {
        ProcessorFut {
            fut,
            cancelled: false,
        }
    }

    pub(crate) fn cancel(self: std::pin::Pin<&mut Self>) {
        *self.project().cancelled = true;
    }
}
/// Convenience conversion to convert a Processor into a ProcessorFut.
/// Polling this future is always a no-op.
impl<State, Role, RootMethod: traits::method::Branch, C: io::Connection, Handler>
    From<Processor<State, Role, RootMethod, C, Handler>>
    for ProcessorFut<
        Pending<
            Result<
                ProcessorTransition<
                    State,
                    Role,
                    C,
                    ReplyPrimed<NotApplicable, C::SendStream, Handler>,
                >,
                Infallible,
            >,
        >,
    >
{
    fn from(_value: Processor<State, Role, RootMethod, C, Handler>) -> Self {
        ProcessorFut::new(pending())
    }
}

impl<Fut, Output> Future for ProcessorFut<Fut>
where
    Fut: Future<Output = Output>,
{
    type Output = Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.project();
        if *this.cancelled {
            std::task::Poll::Pending
        } else {
            this.fut.poll(cx)
        }
    }
}

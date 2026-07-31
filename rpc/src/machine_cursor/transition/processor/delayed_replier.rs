use std::{
    convert::Infallible,
    mem::transmute,
    pin::{Pin, pin},
    task::Poll,
};

use futures::{FutureExt, ready};

use crate::{
    RpcMessage,
    machine_cursor::transition::transition_request_method,
    traits::{self, method},
    transport::ReplyHelper,
};

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

pub struct DelayedReceipt<Method: crate::Method> {
    to_send: transition_request_method::Res,
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

    pub fn res(&self) -> &Method::Res {
        &self.res
    }
}

impl<Method: crate::Method> DelayedReceipt<Method>
where
    Method::Res: RpcMessage,
{
    pub(crate) fn new(res: Method::Res) -> Result<Self, minicbor::encode::Error<Infallible>> {
        Ok(Self {
            to_send: transition_request_method::Res::new(&res, false)?,
            res,
            _marker: method::Wrapper::new(),
        })
    }
}

impl<Method: crate::Method> DelayedReceipt<Method> {
    pub fn finalize<Sender: futures::AsyncWrite + Unpin>(
        self,
        sender: Sender,
        in_tiebreak: bool,
    ) -> (Method::Res, FinalizeFuture<Sender>) {
        let Self {
            mut to_send, res, ..
        } = self;
        to_send.set_in_tiebreak(in_tiebreak);
        (
            res,
            FinalizeFuture {
                state: Some(FinalizeFutureState::Sender(sender)),
                to_send: Some(to_send),
                in_tiebreak,
            },
        )
    }
}

enum FinalizeFutureState<Sender: futures::AsyncWrite + Unpin> {
    Sender(Sender),
    Writer(minicbor_io::AsyncWriter<Sender>),
}

pub struct FinalizeFuture<Sender: futures::AsyncWrite + Unpin> {
    to_send: Option<transition_request_method::Res>,
    in_tiebreak: bool,
    state: Option<FinalizeFutureState<Sender>>,
}

impl<Sender: futures::AsyncWrite + Unpin> FinalizeFuture<Sender> {
    /// Returns whether or not the set operation was successful.
    #[must_use]
    pub fn set_in_tiebreak(&mut self, in_tiebreak: bool) -> bool {
        let Some(to_send) = &mut self.to_send else {
            return false;
        };

        to_send.set_in_tiebreak(in_tiebreak);
        true
    }
}

impl<Sender: futures::AsyncWrite + Unpin> Future for FinalizeFuture<Sender> {
    type Output = Result<(), minicbor_io::Error>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = &mut *self;
        let out = match this.state.take().unwrap() {
            FinalizeFutureState::Sender(sender) => {
                let mut writer = minicbor_io::AsyncWriter::new(sender);
                let res = pin!(writer.write(this.to_send.take().unwrap())).poll(cx);
                this.state = Some(FinalizeFutureState::Writer(writer));
                ready!(res).map(|_| ())
            }
            FinalizeFutureState::Writer(mut async_writer) => {
                ready!(pin!(async_writer.sync()).poll_unpin(cx))
            }
        };

        Poll::Ready(out)
    }
}

impl<Method: crate::Method, RootMethod> ReplyHelper<Method, RootMethod> for DelayedReplier<Method> {
    type Error = minicbor::encode::Error<Infallible>;
    type Receipt<M: crate::Method> = DelayedReceipt<M>;

    async fn reply<Error>(
        self,
        res: Method::Res,
    ) -> Result<Self::Receipt<Method>, crate::traits::HandleError<Self::Error, Error>>
    where
        Method::Res: crate::RpcMessage,
    {
        DelayedReceipt::new(res).map_err(crate::traits::HandleError::Replier)
    }

    async fn reply_with<
        NewMethod: crate::Method,
        Handler: traits::Handler<RootMethod, NewMethod>,
    >(
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

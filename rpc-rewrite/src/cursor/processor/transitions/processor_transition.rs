use std::marker::PhantomData;

use crate::traits::{self, Receipt, io::BytesWriteStream};

pub struct ProcessorTransition<State, Role, C: traits::Connection, T>(
    PhantomData<(State, Role)>,
    C,
    T,
);

impl<State, Role, C: traits::Connection, T> ProcessorTransition<State, Role, C, T> {
    pub fn conn(&self) -> &C {
        &self.1
    }
}

pub struct ReplyPrimed<Res, SendStream: BytesWriteStream, NextHandler> {
    receipt: super::delayed_replier::Receipt<Res, SendStream>,
    next_handler: NextHandler,
}

impl<State, Role, C: traits::Connection, Res, SendStream: BytesWriteStream, NextHandler>
    ProcessorTransition<State, Role, C, ReplyPrimed<Res, SendStream, NextHandler>>
{
    pub(crate) fn new(
        connection: C,
        receipt: super::delayed_replier::Receipt<Res, SendStream>,
        next_handler: NextHandler,
    ) -> Self {
        Self(
            PhantomData,
            connection,
            ReplyPrimed {
                receipt,
                next_handler,
            },
        )
    }

    pub(crate) fn res(&self) -> &Res {
        self.2.receipt.res()
    }

    pub(crate) async fn reply(
        self,
    ) -> Result<
        (
            (Res, NextHandler),
            ProcessorTransition<State, Role, C, Finished>,
        ),
        SendStream::Error,
    > {
        let ReplyPrimed {
            receipt,
            next_handler,
        } = self.2;

        let res = receipt.finalize().await?;

        let finished = ProcessorTransition(self.0, self.1, Finished(()));
        Ok(((res, next_handler), finished))
    }

    pub(crate) fn set_in_tiebreak(&mut self, in_tiebreak: bool) {
        self.2.receipt.set_in_tiebreak(in_tiebreak);
    }
}

pub struct Finished(());

impl<State, Role, C: traits::Connection> ProcessorTransition<State, Role, C, Finished> {
    pub(crate) fn into_conn(self) -> C {
        self.1
    }
}

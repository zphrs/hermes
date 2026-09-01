use std::marker::PhantomData;

use crate::{
    cursor::processor::transitions::delayed_replier::TransitionReply,
    traits::{Connection, Method, io::BytesReadStream, method::ResOf},
};

pub struct RequesterTransition<State, Role, C: Connection, T>(PhantomData<(State, Role)>, C, T);

impl<State, Role, C: Connection, T> RequesterTransition<State, Role, C, T> {
    pub(crate) fn conn(&self) -> &C {
        &self.1
    }
}

/// state of waiting for a response
pub struct Sent<'buf, RootRequest, Recv: BytesReadStream, M> {
    recv: Recv,
    root_request: RootRequest,
    buf: &'buf mut Vec<u8>,
    _marker: PhantomData<M>,
}

#[derive(Debug, thiserror::Error)]
pub enum RecvError<Recv: BytesReadStream> {
    #[error("decode: {0}")]
    Decode(#[from] minicbor::decode::Error),
    #[error("read: {0}")]
    Read(Recv::Error),
}

impl<'buf, State, Role, C: Connection, RootRequest, Recv: BytesReadStream, M: Method>
    RequesterTransition<State, Role, C, Sent<'buf, RootRequest, Recv, M>>
{
    pub(crate) fn new_sent(
        connection: C,
        recv: Recv,
        root_request: RootRequest,
        buf: &'buf mut Vec<u8>,
    ) -> Self {
        Self(
            PhantomData,
            connection,
            Sent {
                recv,
                root_request,
                buf,
                _marker: PhantomData,
            },
        )
    }
    pub(crate) fn res(&self) -> &RootRequest {
        &self.2.root_request
    }

    pub(crate) async fn recv(
        self,
    ) -> Result<
        (
            TransitionReply<ResOf<'buf, M>>,
            RequesterTransition<State, Role, C, Finished>,
        ),
        RecvError<Recv>,
    >
    where
        ResOf<'buf, M>: minicbor::Decode<'buf, ()>,
    {
        crate::io::read_to_end(self.2.recv, self.2.buf, usize::MAX)
            .await
            .map_err(RecvError::Read)?;
        // recv dropped here
        let res: TransitionReply<ResOf<'_, M>> = minicbor::decode(self.2.buf)?;
        Ok((res, RequesterTransition(self.0, self.1, Finished(()))))
    }
}

pub struct Finished(());

impl<State, Role, C: Connection> RequesterTransition<State, Role, C, Finished> {
    pub fn into_conn(self) -> C {
        self.1
    }
}

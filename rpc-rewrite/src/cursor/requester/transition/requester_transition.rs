use std::marker::PhantomData;

use crate::{
    cursor::processor::transition::delayed_replier::TransitionReply,
    io::{BytesReadStream, Connection},
    method::{Method, ResOf},
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
    _root_request: RootRequest,
    buf: &'buf mut Vec<u8>,
    _marker: PhantomData<M>,
}

pub type Entrypoint<'buf, State, Role, C, RootRequest, M> =
    RequesterTransition<State, Role, C, Sent<'buf, RootRequest, <C as Connection>::RecvStream, M>>;

impl<'buf, State, Role, C: Connection, RootRequest, M: Method>
    RequesterTransition<State, Role, C, Sent<'buf, RootRequest, C::RecvStream, M>>
{
    pub(crate) fn new_sent(
        connection: C,
        recv: C::RecvStream,
        root_request: RootRequest,
        buf: &'buf mut Vec<u8>,
    ) -> Self {
        Self(
            PhantomData,
            connection,
            Sent {
                recv,
                _root_request: root_request,
                buf,
                _marker: PhantomData,
            },
        )
    }
    /// Resolves to `None` if the peer explicitly rejected the request. A peer
    /// that finishes the stream without replying (e.g. because it dropped its
    /// processor) surfaces as [`read::Error::Empty`](crate::io::read::Error::Empty).
    pub(crate) async fn receive(
        self,
    ) -> Result<
        (
            Option<TransitionReply<ResOf<'buf, M>>>,
            RequesterTransition<State, Role, C, Finished>,
        ),
        crate::io::read::Error<C::RecvStream>,
    >
    where
        ResOf<'buf, M>: minicbor::Decode<'buf, ()>,
    {
        let res: Option<TransitionReply<ResOf<'buf, M>>> =
            crate::io::read::read(self.2.buf, self.2.recv).await?;
        // recv dropped here
        Ok((res, RequesterTransition(self.0, self.1, Finished(()))))
    }
}

pub struct Finished(());

impl<State, Role, C: Connection> RequesterTransition<State, Role, C, Finished> {
    pub(crate) fn into_conn(self) -> C {
        self.1
    }
}

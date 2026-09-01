pub mod requester_transition;

use super::Requester;
use crate::{
    cursor::requester::transition::requester_transition::Sent,
    traits::{
        self,
        io::BytesWriteStream,
        method::{self, ReqOf, ResOf},
    },
};

use std::convert::Infallible;

pub use requester_transition::RequesterTransition;

#[derive(Debug, thiserror::Error)]
pub enum RequestTransitionError<C: traits::io::Connection> {
    #[error("opening: {0}")]
    Open(C::OpenError),
    #[error("io: {0}")]
    Write(<C::SendStream as BytesWriteStream>::Error),
    #[error("encode: {0}")]
    Encode(#[from] minicbor::encode::Error<Infallible>),
}

impl<State, Role, RootMethod: crate::traits::Method, C: traits::io::Connection>
    Requester<State, Role, RootMethod, C>
{
    pub async fn request_transition<
        'req,
        'buf,
        M: method::Descendant<RootMethod> + method::Leaf + method::Transitions,
    >(
        self,
        request: ReqOf<'req, M>,
        read_into: &'buf mut Vec<u8>,
    ) -> Result<
        RequesterTransition<State, Role, C, Sent<'buf, ReqOf<'req, RootMethod>, C::RecvStream, M>>,
        RequestTransitionError<C>,
    >
    where
        ReqOf<'req, RootMethod>: minicbor::CborLen<()> + minicbor::Encode<()>,
        ResOf<'buf, M>: minicbor::Decode<'buf, ()>,
    {
        let (send, recv) = self
            .connection
            .open_stream()
            .await
            .map_err(RequestTransitionError::Open)?;
        let root_request = M::req_to_parent(request);
        read_into.clear();
        read_into.reserve(minicbor::len(&root_request));
        let mut buf = Vec::with_capacity(minicbor::len(&root_request));
        minicbor::encode::<&ReqOf<RootMethod>, _>(&root_request, &mut buf)?;
        crate::io::write_all(send, buf.into(), false)
            .await
            .map_err(RequestTransitionError::Write)?;
        // send dropped here
        read_into.clear();
        Ok(RequesterTransition::new_sent(
            self.connection,
            recv,
            root_request,
            read_into,
        ))
    }
}

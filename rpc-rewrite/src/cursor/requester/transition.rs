pub mod requester_transition;

use super::Requester;
use crate::{
    cursor::requester::transition::requester_transition::Sent,
    io::{self, Connection},
    method::{self, ReqOf, ResOf},
};

pub use requester_transition::RequesterTransition;

#[derive(Debug, thiserror::Error)]
pub enum RequestTransitionError<C: Connection> {
    #[error("could not open stream")]
    Open(#[source] C::OpenError),
    #[error("could not write request")]
    Write(#[from] io::write::Error<C::SendStream>),
}

impl<State, Role, RootMethod: method::Method, C: Connection> Requester<State, Role, RootMethod, C> {
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
        crate::io::write(&root_request, send).await?;

        Ok(RequesterTransition::new_sent(
            self.connection,
            recv,
            root_request,
            read_into,
        ))
    }
}

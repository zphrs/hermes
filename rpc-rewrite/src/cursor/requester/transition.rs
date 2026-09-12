pub mod requester_transition;

use super::Requester;
use crate::{
    cursor::{
        self,
        processor::{Processor, transition::delayed_replier::TransitionReply},
        requester::transition::requester_transition::Sent,
        transition::CursorCredit,
    },
    io::{self, Connection},
    marker::{NotApplicable, not_applicable},
    method::{self, LeafTransition, ReqOf, ResOf},
};

pub use requester_transition::RequesterTransition;

#[derive(Debug, thiserror::Error)]
pub enum RequestConcurrentTransitionError<C: Connection> {
    #[error("could not open stream")]
    Open(#[source] C::OpenError),
    #[error("could not write transition request")]
    Write(#[from] io::write::Error<C::SendStream>),
}

#[derive(Debug, thiserror::Error)]
pub enum RequestTransitionError<C: Connection> {
    #[error("could not open stream")]
    Open(#[source] C::OpenError),
    #[error("could not write transition request")]
    Write(#[from] io::write::Error<C::SendStream>),
    #[error("could not read approval response")]
    Read(#[from] io::read::Error<C::RecvStream>),
    #[error("in tiebreak flag unexpectedly set")]
    InTiebreak,
}

impl<C: Connection> From<RequestConcurrentTransitionError<C>> for RequestTransitionError<C> {
    fn from(value: RequestConcurrentTransitionError<C>) -> Self {
        match value {
            RequestConcurrentTransitionError::Open(error) => RequestTransitionError::Open(error),
            RequestConcurrentTransitionError::Write(error) => RequestTransitionError::Write(error),
        }
    }
}

impl<State, Role, RootMethod: method::Method, C: Connection> Requester<State, Role, RootMethod, C> {
    pub async fn request_concurrent_transition<
        'req,
        'buf,
        M: method::Descendant<RootMethod> + method::OfType<LeafTransition>,
    >(
        self,
        request: ReqOf<'req, M>,
        read_into: &'buf mut Vec<u8>,
    ) -> Result<
        RequesterTransition<State, Role, C, Sent<'buf, ReqOf<'req, RootMethod>, C::RecvStream, M>>,
        RequestConcurrentTransitionError<C>,
    >
    where
        ReqOf<'req, RootMethod>: minicbor::CborLen<()> + minicbor::Encode<()>,
        ResOf<'buf, M>: minicbor::Decode<'buf, ()>,
    {
        let (send, recv) = self
            .connection
            .open_stream()
            .await
            .map_err(RequestConcurrentTransitionError::Open)?;
        let root_request = M::req_to_parent(request);
        crate::io::write(&root_request, send).await?;

        Ok(RequesterTransition::new_sent(
            self.connection,
            recv,
            root_request,
            read_into,
        ))
    }
    #[expect(private_bounds, reason = "role")]
    pub async fn request_transition<
        'req,
        'buf,
        M: method::Descendant<RootMethod> + method::OfType<LeafTransition>,
    >(
        self,
        request: ReqOf<'req, M>,
        read_into: &'buf mut Vec<u8>,
        processor: Processor<State, Role, NotApplicable, C, not_applicable::Handler>,
    ) -> Result<(ResOf<'buf, M>, CursorCredit<State, Role, C>), RequestTransitionError<C>>
    where
        ReqOf<'req, RootMethod>: minicbor::CborLen<()> + minicbor::Encode<()>,
        ResOf<'buf, M>: minicbor::Decode<'buf, ()>,
        Role: cursor::role::Sealed,
    {
        drop(processor);
        let requester_transition = self
            .request_concurrent_transition::<M>(request, read_into)
            .await?;

        // next::with_requester_transition(requester_transition, pin!(processor.into())).await?;
        let recv = requester_transition.receive().await?;
        let (TransitionReply { reply, in_tiebreak }, requester_transition) = recv;
        let connection = requester_transition.into_conn();
        // would ordinarily notify, but no need to do so here
        // because there's no case where we might be tiebreaking
        if in_tiebreak {
            Err(RequestTransitionError::InTiebreak)?
        }
        Ok((reply, CursorCredit::new(connection)))
    }
}

impl<State, Role, RootMethod: method::Method, C: Connection> Requester<State, Role, RootMethod, C> {}

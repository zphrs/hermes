pub mod transition;

use std::{fmt::Debug, marker::PhantomData};

use crate::{
    io::Connection,
    method::{self, Descendant, LeafLoopback, ReqOf, ResOf},
};

pub struct Requester<State, Role, RootMethod: crate::Method, C: Connection> {
    connection: C,
    _marker: PhantomData<(State, Role, RootMethod)>,
}

#[derive(thiserror::Error)]
pub enum RequestLoopbackError<C: Connection> {
    #[error("opening: {0}")]
    Open(C::OpenError),
    #[error("request: {0}")]
    Request(#[from] crate::io::request::Error<C>),
}

impl<C: Connection> std::fmt::Debug for RequestLoopbackError<C>
where
    C::OpenError: Debug,
    crate::io::request::Error<C>: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(arg0) => f.debug_tuple("Open").field(arg0).finish(),
            Self::Request(arg0) => f.debug_tuple("Request").field(arg0).finish(),
        }
    }
}

impl<State, Role, RootMethod: crate::Method, C: Connection> Requester<State, Role, RootMethod, C> {
    pub(crate) fn new(connection: C) -> Self {
        Self {
            connection,
            _marker: PhantomData,
        }
    }

    pub(crate) fn conn(&self) -> &C {
        &self.connection
    }

    pub async fn request_loopback<
        'req,
        'buf,
        M: Descendant<RootMethod> + method::OfType<LeafLoopback>,
    >(
        &self,
        request: ReqOf<'req, M>,
        buf: &'buf mut Vec<u8>,
    ) -> Result<ResOf<'buf, M>, RequestLoopbackError<C>>
    where
        ReqOf<'req, RootMethod>: minicbor::CborLen<()> + minicbor::Encode<()>,
        ResOf<'buf, M>: minicbor::Decode<'buf, ()>,
    {
        let stream = self
            .connection
            .open_stream()
            .await
            .map_err(RequestLoopbackError::Open)?;
        let res: ResOf<'buf, M> =
            crate::io::request::<RootMethod, M, C>(buf, request, stream).await?;
        Ok(res)
    }
}

use crate::machine_cursor::transition::processor::Entrypoint;
use crate::machine_cursor::transition::processor::ProcessorTransition;
use crate::method::is_leaf;

use maxlen::MaxLen;

use crate::{
    Caller,
    machine_cursor::Processor,
    traits::method::{can_transition, not_applicable::NotApplicable},
    transport::CallerExt,
};

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct Notification;

pub struct Method;

impl crate::Method for Method {
    type Req = Notification;

    type Res = NotApplicable;

    type CanTransition = can_transition::False;

    type IsLeaf = is_leaf::True;
}

impl<
    ProcessorMethod: crate::Method,
    Conn: crate::transport::Client,
    State: crate::state::Prioritized,
    Role: crate::state::Role,
> ProcessorSacrifice for ProcessorTransition<Entrypoint<State, ProcessorMethod, Role, Conn>>
{
}

pub trait ProcessorSacrifice {
    fn sacrifice<Conn: crate::transport::Connection>(
        self,
        conn: &mut Conn,
    ) -> impl Future<Output = Result<(), crate::CallerError<<Conn as Caller>::Error>>>
    where
        Self: Sized,
    {
        conn.notify::<Method, Notification>(Notification)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AssertSacrificeError<Client> {
    #[error("while accepting stream: {0}")]
    AcceptStream(Client),
    #[error("handler: {0}")]
    Handler(#[from] minicbor_io::Error),
}

pub async fn assert_remote_sacrifice<Conn: crate::transport::Connection>(
    conn: &mut Conn,
) -> Result<(), AssertSacrificeError<<Conn as crate::transport::Client>::Error>> {
    let mut stream = conn
        .accept_stream()
        .await
        .map_err(AssertSacrificeError::AcceptStream)?;
    tracing::warn!("opened notification stream");
    conn.handle_one_notification::<Method>(&mut stream).await?;
    tracing::warn!("handled notification");

    Ok(())
}

impl<
    State: crate::State,
    Role: crate::state::Role,
    RootMethod: crate::Method,
    Client: crate::transport::Client,
    H: crate::Handler<RootMethod, RootMethod>,
> ProcessorSacrifice for Processor<State, Role, RootMethod, Client, H>
{
}

pub struct ToSacrifice();

impl ToSacrifice {
    pub(crate) fn new() -> Self {
        Self()
    }
}

impl ProcessorSacrifice for ToSacrifice {}

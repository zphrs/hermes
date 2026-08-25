use crate::machine_cursor::processor::EventualTransitionRequest;
use crate::machine_cursor::transition::processor::ProcessorTransition;
use crate::machine_cursor::transition::processor::StageOne;
use crate::method::is_leaf;
use crate::method::not_applicable;
use crate::state;
use crate::traits::state::StateTypeIdExt as _;
use crate::transport::ClientExt as _;

use maxlen::MaxLen;
use tracing::trace;

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

pub(crate) trait ProcessorSacrifice {
    fn sacrifice(self) -> ToSacrifice
    where
        Self: Sized,
    {
        ToSacrifice(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AssertSacrificeError<Client> {
    #[error("while accepting stream: {0}")]
    AcceptStream(Client),
    #[error("handler: {0}")]
    Handler(#[from] minicbor_io::Error),
}

#[expect(private_bounds, reason = "for role")]
pub async fn assert_remote_sacrifice<
    OldState: crate::State,
    Role: state::Role,
    Conn: crate::transport::Connection,
>(
    conn: &mut Conn,
) -> Result<(), AssertSacrificeError<<Conn as crate::transport::Client>::Error>> {
    if OldState::remote_handles_type_id::<Role>() == not_applicable::TYPE_ID {
        trace!("skipping waiting for remote sacrifice");
        return Ok(());
    }
    let mut stream = conn
        .accept_stream()
        .await
        .map_err(AssertSacrificeError::AcceptStream)?;
    tracing::debug!("opened notification stream");
    conn.handle_one_notification::<Method>(&mut stream).await?;
    tracing::debug!("handled notification");

    Ok(())
}

impl<
    ProcessorMethod: crate::Method,
    Conn: crate::transport::Client,
    State: crate::state::Prioritized,
    Role: crate::state::Role,
> ProcessorSacrifice for ProcessorTransition<StageOne<State, ProcessorMethod, Role, Conn>>
{
}

impl<Fut> ProcessorSacrifice for EventualTransitionRequest<Fut> {}

impl<
    'h,
    State: crate::State,
    Role: crate::state::Role,
    RootMethod: crate::Method,
    Client: crate::transport::Client,
    H: crate::Handler<RootMethod, RootMethod>,
> ProcessorSacrifice for Processor<'h, State, Role, RootMethod, Client, H>
{
}

pub struct ToSacrifice(());

impl ToSacrifice {
    pub fn sacrifice<Conn: crate::transport::Connection>(
        self,
        conn: &mut Conn,
    ) -> impl Future<Output = Result<(), crate::CallerError<<Conn as Caller>::Error>>>
    where
        Self: Sized,
    {
        conn.notify::<Method, Method>(Notification)
    }
}

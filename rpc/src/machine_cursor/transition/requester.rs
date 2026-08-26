//! 1. Send transition request
//! 2. await response from transition request to turn into a receipt
//! 3. With the handler/pendingTransitionRequest, if the receipt has the tiebreak
//!    flag set:
//!     1. if we have the handler then wait for the handler to turn into a
//!        pendingTransitionRequest
//!     2. continue with the receipt because the other side tiebroke in our
//!        direction. Maybe do a debug assert that we'd tiebreak in the same
//!        direction.
//! 3. Else:
//!     1. continue with the receipt
//! 2. with the receipt, send a notification that we've transitioned fully to the
//!    awaiting processor.
//!
//! While waiting for the transition request to turn into a receipt, also wait
//! for the processor to receive a transition request. If the processor does receive a transition request then it will tiebreak. If our request wins then continue waiting for the transition request to turn into a receipt.

use crate::{
    RpcMessage,
    method::{CanTransition, FromDescendant, is_leaf},
    traits::state::priority::PrioritizedUnsafeExt,
};
pub(super) mod processor_sacrifice;
use std::marker::PhantomData;

pub use processor_sacrifice::{AssertSacrificeError, ToSacrifice, assert_remote_sacrifice};
mod request_transition;

pub use request_transition::{StageOne, StageZero, TransitionReceipt};

pub struct RequesterTransition<OldState, Stage> {
    stage: Stage,
    _marker: PhantomData<OldState>,
}

pub type RequesterTransitionClientEntrypoint<State, TransitionMethod, Connection> =
    RequesterTransition<
        State,
        StageZero<
            <State as crate::State>::ServerHandles,
            TransitionMethod,
            state::role::Client,
            Connection,
        >,
    >;

pub type RequesterTransitionServerEntrypoint<State, TransitionMethod, Connection> =
    RequesterTransition<
        State,
        StageZero<
            <State as crate::State>::ClientHandles,
            TransitionMethod,
            state::role::Server,
            Connection,
        >,
    >;

impl<State, Stage> RequesterTransition<State, Stage> {
    pub(crate) fn into_inner(self) -> Stage {
        self.stage
    }
}

use crate::{
    CallerError, Handler, MachineCursor,
    machine_cursor::{Processor, transition::requester::processor_sacrifice::ProcessorSacrifice},
    state::{self, Prioritized},
};

#[expect(private_bounds, reason = "for role")]
pub enum Need<
    State: crate::state::Prioritized,
    TransitionMethod: crate::Method,
    Role: crate::state::Role,
    Caller: crate::Caller,
> {
    Processor(RequesterTransition<State, NeedProcessor<TransitionMethod::Res, Role, Caller>>),
    IncomingTransitionRequest(
        #[expect(private_interfaces, reason = "for role")]
        RequesterTransition<
            State,
            NeedIncomingTransitionRequest<TransitionMethod::Res, Role, Caller, State::Priority>,
        >,
    ),
}

#[expect(private_bounds, reason = "for role")]
impl<
    State: crate::state::Prioritized,
    TransitionMethod: crate::Method,
    Role: crate::state::Role,
    Caller: crate::Caller,
> Need<State, TransitionMethod, Role, Caller>
{
    #[allow(clippy::type_complexity)]
    pub fn try_into_need_processor(
        self,
    ) -> Result<RequesterTransition<State, NeedProcessor<TransitionMethod::Res, Role, Caller>>, Self>
    {
        if let Self::Processor(processor) = self {
            Ok(processor)
        } else {
            Err(self)
        }
    }
    pub fn assert_need_processor(
        self,
    ) -> RequesterTransition<State, NeedProcessor<TransitionMethod::Res, Role, Caller>> {
        self.try_into()
            .ok()
            .expect("need variant should be Need::Processor")
    }
    #[allow(private_interfaces, clippy::type_complexity)]
    pub fn try_into_need_incoming_transition_request(
        self,
    ) -> Result<
        RequesterTransition<
            State,
            NeedIncomingTransitionRequest<TransitionMethod::Res, Role, Caller, State::Priority>,
        >,
        Self,
    > {
        if let Self::IncomingTransitionRequest(need_incoming) = self {
            Ok(need_incoming)
        } else {
            Err(self)
        }
    }
    #[allow(private_interfaces)]
    pub fn assert_need_incoming_transition_request(
        self,
    ) -> RequesterTransition<
        State,
        NeedIncomingTransitionRequest<
            TransitionMethod::Res,
            Role,
            Caller,
            <State as Prioritized>::Priority,
        >,
    > {
        self.try_into()
            .ok()
            .expect("need variant should be Need::IncomingTransitionRequest")
    }
}

impl<
    State: crate::state::Prioritized,
    TransitionMethod: crate::Method,
    Role: crate::state::Role,
    Caller: crate::Caller,
> TryFrom<Need<State, TransitionMethod, Role, Caller>>
    for RequesterTransition<State, NeedProcessor<TransitionMethod::Res, Role, Caller>>
{
    type Error = Need<State, TransitionMethod, Role, Caller>;

    fn try_from(value: Need<State, TransitionMethod, Role, Caller>) -> Result<Self, Self::Error> {
        value.try_into_need_processor()
    }
}

impl<
    State: crate::state::Prioritized,
    TransitionMethod: crate::Method,
    Role: crate::state::Role,
    Caller: crate::Caller,
> TryFrom<Need<State, TransitionMethod, Role, Caller>>
    for RequesterTransition<
        State,
        NeedIncomingTransitionRequest<
            TransitionMethod::Res,
            Role,
            Caller,
            <State as crate::state::Prioritized>::Priority,
        >,
    >
{
    type Error = Need<State, TransitionMethod, Role, Caller>;

    fn try_from(value: Need<State, TransitionMethod, Role, Caller>) -> Result<Self, Self::Error> {
        value.try_into_need_incoming_transition_request()
    }
}

#[expect(private_bounds, reason = "for role")]
pub struct NeedProcessor<Res, Role: crate::state::Role, Caller: crate::Caller>(
    TransitionReceipt<Res, Role, Caller>,
);

impl<Res, Role: crate::state::Role, Caller: crate::Caller>
    From<TransitionReceipt<Res, Role, Caller>> for NeedProcessor<Res, Role, Caller>
{
    fn from(value: TransitionReceipt<Res, Role, Caller>) -> Self {
        Self(value)
    }
}
pub(crate) struct NeedIncomingTransitionRequest<
    Res,
    Role: crate::state::Role,
    Caller: crate::Caller,
    Priority,
> {
    pub _receipt: TransitionReceipt<Res, Role, Caller>,
    pub _priority: Priority,
}

#[expect(
    private_bounds,
    reason = "role trait is private to force role to be either Server or Client"
)]
impl<
    State,
    RootMethod: crate::Method + CanTransition,
    TransitionMethod: crate::Method<IsLeaf = is_leaf::True>,
    Role: crate::state::Role,
    Caller: crate::transport::Caller,
> RequesterTransition<State, StageZero<RootMethod, TransitionMethod, Role, Caller>>
{
    pub fn new(transition: StageZero<RootMethod, TransitionMethod, Role, Caller>) -> Self {
        Self {
            stage: transition,
            _marker: PhantomData,
        }
    }

    pub(crate) async fn next_while_handling(
        self,
    ) -> Result<
        RequesterTransition<State, StageOne<RootMethod, TransitionMethod, Role, Caller>>,
        Caller::Error,
    >
    where
        RootMethod::Req: crate::RpcMessage,
        TransitionMethod: crate::Method<IsLeaf = is_leaf::True> + CanTransition,
        RootMethod: FromDescendant<TransitionMethod>,
        TransitionMethod::Res: RpcMessage,
    {
        let (req, role, caller) = self.stage.into_parts();
        Ok(StageOne::new(req, role, caller).await?.into())
    }
}

pub mod fast_path {
    use crate::{
        CallerError,
        machine_cursor::transition::{StageZero, requester::RequesterTransition},
        method::{CanTransition, can_transition, is_leaf},
        state::{Prioritized, role},
    };
    // for client requester
    impl<
        State: crate::State,
        RootMethod: crate::Method + CanTransition + crate::method::FromDescendant<TransitionMethod>,
        TransitionMethod: crate::Method<IsLeaf = is_leaf::True> + CanTransition,
        Caller: crate::transport::Caller,
    > RequesterTransition<State, StageZero<RootMethod, TransitionMethod, role::Client, Caller>>
    where
        <TransitionMethod as crate::Method>::Res: crate::RpcMessage,
        State: Prioritized,
        TransitionMethod::Res: crate::RpcMessage,
        RootMethod::Req: crate::RpcMessage,
        <State as crate::State>::ClientHandles:
            crate::Method<CanTransition = can_transition::False>,
    {
        /// fast path
        pub async fn next(
            self,
        ) -> Result<
            RequesterTransition<
                State,
                super::NeedProcessor<
                    <TransitionMethod as crate::Method>::Res,
                    role::Client,
                    Caller,
                >,
            >,
            CallerError<<Caller as crate::Caller>::Error>,
        > {
            let next = self
                .next_while_handling()
                .await
                .map_err(CallerError::Transport)?;
            let res = next.next_when_handling_not_applicable().await?;
            Ok(res)
        }
    }
    // for server requester
    impl<
        State: crate::State,
        RootMethod: crate::Method + CanTransition + crate::method::FromDescendant<TransitionMethod>,
        TransitionMethod: crate::Method<IsLeaf = is_leaf::True> + CanTransition,
        Caller: crate::transport::Caller,
    > RequesterTransition<State, StageZero<RootMethod, TransitionMethod, role::Server, Caller>>
    where
        <TransitionMethod as crate::Method>::Res: crate::RpcMessage,
        State: Prioritized,
        TransitionMethod::Res: crate::RpcMessage,
        RootMethod::Req: crate::RpcMessage,
        <State as crate::State>::ServerHandles:
            crate::Method<CanTransition = can_transition::False>,
    {
        /// fast path
        pub async fn next(
            self,
        ) -> Result<
            RequesterTransition<
                State,
                super::NeedProcessor<
                    <TransitionMethod as crate::Method>::Res,
                    role::Server,
                    Caller,
                >,
            >,
            CallerError<<Caller as crate::Caller>::Error>,
        > {
            let next = self
                .next_while_handling()
                .await
                .map_err(CallerError::Transport)?;
            let res = next.next_when_handling_not_applicable().await?;
            Ok(res)
        }
    }
}

impl<
    State,
    RootMethod: crate::Method,
    TransitionMethod: crate::Method,
    Role: crate::state::Role,
    Caller: crate::transport::Caller,
> From<StageOne<RootMethod, TransitionMethod, Role, Caller>>
    for RequesterTransition<State, StageOne<RootMethod, TransitionMethod, Role, Caller>>
{
    fn from(value: StageOne<RootMethod, TransitionMethod, Role, Caller>) -> Self {
        Self {
            stage: value,
            _marker: PhantomData,
        }
    }
}

#[expect(private_bounds, reason = "for role")]
impl<
    State,
    RootMethod: crate::Method,
    TransitionMethod: crate::Method,
    Role: crate::state::Role,
    Caller: crate::transport::Caller,
> RequesterTransition<State, StageOne<RootMethod, TransitionMethod, Role, Caller>>
{
    pub async fn next(
        self,
    ) -> Result<
        Need<State, TransitionMethod, Role, Caller>,
        CallerError<<Caller as crate::Caller>::Error>,
    >
    where
        <TransitionMethod as crate::Method>::Res: crate::RpcMessage,
        State: Prioritized,
        TransitionMethod::Res: crate::RpcMessage,
        RootMethod::Req: crate::RpcMessage,
    {
        let (req, receipt) = self
            .stage
            .await
            .map_err(|e| CallerError::try_from(e).unwrap())?;
        let (res, receipt) = receipt.extract_result();
        let (in_transition, res) = res.into_parts();

        let receipt = receipt.insert_result(res);

        Ok(if in_transition {
            let priority = unsafe { State::requester_priority::<Role, _>(&req) };

            Need::IncomingTransitionRequest(RequesterTransition::from(
                NeedIncomingTransitionRequest {
                    _receipt: receipt,
                    _priority: priority,
                },
            ))
        } else {
            Need::Processor(RequesterTransition::from(NeedProcessor(receipt)))
        })
    }
}
mod stage_one_fast_path {
    use crate::{
        CallerError,
        machine_cursor::transition::requester::{NeedProcessor, RequesterTransition, StageOne},
        method::can_transition,
        state::{Prioritized, role},
    };

    // for client requester
    impl<
        State: crate::State,
        RootMethod: crate::Method,
        TransitionMethod: crate::Method,
        Caller: crate::transport::Caller,
    > RequesterTransition<State, StageOne<RootMethod, TransitionMethod, role::Client, Caller>>
    {
        pub async fn next_when_handling_not_applicable(
            self,
        ) -> Result<
            RequesterTransition<State, NeedProcessor<TransitionMethod::Res, role::Client, Caller>>,
            CallerError<<Caller as crate::Caller>::Error>,
        >
        where
            <TransitionMethod as crate::Method>::Res: crate::RpcMessage,
            State: Prioritized,
            TransitionMethod::Res: crate::RpcMessage,
            RootMethod::Req: crate::RpcMessage,
            <State as crate::State>::ClientHandles:
                crate::Method<CanTransition = can_transition::False>,
        {
            let (_req, receipt) = self
                .stage
                .await
                .map_err(|e| CallerError::try_from(e).unwrap())?;
            let (res, receipt) = receipt.extract_result();
            let (in_transition, res) = res.into_parts();

            let receipt = receipt.insert_result(res);

            Ok(if in_transition {
                unreachable!()
            } else {
                RequesterTransition::from(NeedProcessor(receipt))
            })
        }
    }
    // for server requester
    impl<
        State,
        RootMethod: crate::Method,
        TransitionMethod: crate::Method,
        Caller: crate::transport::Caller,
    > RequesterTransition<State, StageOne<RootMethod, TransitionMethod, role::Server, Caller>>
    {
        pub async fn next_when_handling_not_applicable(
            self,
        ) -> Result<
            RequesterTransition<State, NeedProcessor<TransitionMethod::Res, role::Server, Caller>>,
            CallerError<<Caller as crate::Caller>::Error>,
        >
        where
            <TransitionMethod as crate::Method>::Res: crate::RpcMessage,
            State: Prioritized,
            TransitionMethod::Res: crate::RpcMessage,
            RootMethod::Req: crate::RpcMessage,
            <State as crate::State>::ServerHandles:
                crate::Method<CanTransition = can_transition::False>,
        {
            let (_req, receipt) = self
                .stage
                .await
                .map_err(|e| CallerError::try_from(e).unwrap())?;
            let (res, receipt) = receipt.extract_result();
            let (in_transition, res) = res.into_parts();

            let receipt = receipt.insert_result(res);

            Ok(if in_transition {
                unreachable!()
            } else {
                RequesterTransition::from(NeedProcessor(receipt))
            })
        }
    }
}

impl<State, TransitionRes, Role: crate::state::Role, Conn: crate::transport::Caller>
    From<NeedProcessor<TransitionRes, Role, Conn>>
    for RequesterTransition<State, NeedProcessor<TransitionRes, Role, Conn>>
{
    fn from(receipt: NeedProcessor<TransitionRes, Role, Conn>) -> Self {
        Self {
            stage: receipt,
            _marker: PhantomData,
        }
    }
}

#[expect(private_bounds, reason = "for role")]
impl<State, TransitionRes, Role: crate::state::Role, Conn: crate::transport::Connection>
    RequesterTransition<State, NeedProcessor<TransitionRes, Role, Conn>>
{
    pub fn extract_res(
        self,
    ) -> (
        TransitionRes,
        RequesterTransition<State, NeedProcessor<(), Role, Conn>>,
    ) {
        let (res, receipt) = self.stage.0.extract_result();
        (res, RequesterTransition::extracted(NeedProcessor(receipt)))
    }
}

#[expect(private_bounds, reason = "for role")]
impl<State, Role: crate::state::Role, Conn: crate::transport::Connection>
    RequesterTransition<State, NeedProcessor<(), Role, Conn>>
{
    fn extracted(stage: NeedProcessor<(), Role, Conn>) -> Self {
        Self {
            stage,
            _marker: PhantomData,
        }
    }
    pub async fn finish<
        'h,
        NewState: crate::State,
        ProcessorMethod: crate::Method,
        H: Handler<ProcessorMethod, ProcessorMethod>,
    >(
        self,
        processor: Processor<'h, State, Role, ProcessorMethod, Conn, H>,
        wrapper: state::Wrapper<NewState>,
    ) -> Result<MachineCursor<NewState, Conn, Role>, CallerError<<Conn as crate::Caller>::Error>>
    where
        State: crate::State,
    {
        let receipt = self.stage.0;
        assert!(
            processor.client() == receipt.connection(),
            "requester and processor must belong to the same connection"
        );
        let (role, conn) = receipt.into_parts::<State>(processor.sacrifice()).await?;

        Ok(MachineCursor::new_with_role(conn, role, wrapper))
    }
}

impl<State, Priority, TransitionRes, Role: crate::state::Role, Caller: crate::Caller>
    From<NeedIncomingTransitionRequest<TransitionRes, Role, Caller, Priority>>
    for RequesterTransition<
        State,
        NeedIncomingTransitionRequest<TransitionRes, Role, Caller, Priority>,
    >
{
    fn from(stage: NeedIncomingTransitionRequest<TransitionRes, Role, Caller, Priority>) -> Self {
        Self {
            stage,
            _marker: PhantomData,
        }
    }
}

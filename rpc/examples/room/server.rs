use anyhow::anyhow;
use futures::{FutureExt, select_biased};

use rpc::{
    in_memory_transport,
    machine_cursor::{
        self, MachineCursorServer,
        transition::{
            processor::{self, ProcessorTransition, ProcessorTransitionServerEntrypoint},
            requester::RequesterTransitionServerEntrypoint,
            tiebreak,
        },
    },
    method::{can_transition, is_leaf},
    state::{Has, role},
    transport::Incoming,
};
use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex},
};
use tracing::{Instrument, debug, span, trace, warn};

use crate::{
    Username,
    server::room::Room,
    states::{
        Entrypoint,
        in_room::{close, from_client::loopback},
    },
};

pub mod user;

pub mod room;

type Rooms = Arc<Mutex<HashMap<Username, Arc<tokio::sync::Mutex<Option<Room>>>>>>;

mod join_room;

mod in_room;

pub async fn handle_incoming(
    conn: in_memory_transport::Connection<&'static str>,
    rooms: Rooms,
) -> anyhow::Result<()> {
    let mut cursor = MachineCursorServer::<Entrypoint, _>::new(conn);
    loop {
        let rooms = rooms.clone();
        let mut handler = join_room::JoinRoomHandler::new(rooms.clone());
        let (processor, requester) = cursor.into_children_with_handler(&mut handler);
        let (res, transition) = processor
            .handle_transition_request()
            .await?
            .next_with_requester(requester)
            .await?
            .extract_res();

        let mut room_mutex_guard = handler.room_lock.take().unwrap();
        let permit = handler.add_user_permit.take().unwrap();
        let room = room_mutex_guard.as_mut().unwrap();

        let room_arc = rooms.lock().unwrap().get(&room.id).unwrap().clone();

        let mut handler =
            in_room::FromClientHandler::new(rooms, room_arc, &room.id, permit.username().clone());
        let loopback_handler = handler.to_loopback_handler();
        let (processor, requester) = transition
            .finish(res.try_extract_wrapper().ok().unwrap())
            .into_children_with_handler(&mut handler);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        {
            room.add_user(
                user::User::new(permit.username().clone(), requester, sender),
                permit,
            )
            .await;
        }
        drop(room_mutex_guard);
        let mut eventual_processor_transition =
            processor.handle_requests::<loopback::Method, _>(loopback_handler);
        let mut fused_receiver = receiver.fuse();
        debug!("server handling requests");
        cursor = select_biased! {
            receiver_transition = fused_receiver => handle_receiver_transition(receiver_transition?, eventual_processor_transition).await?,
            processor_transition = eventual_processor_transition => {
                drop(eventual_processor_transition);
                trace!("handling processor transition");
                handle_processor_transition(processor_transition, handler, fused_receiver).await?
            },
        };
    }
}

async fn handle_receiver_transition(
    receiver: machine_cursor::transition::requester::RequesterTransition<
        crate::states::in_room::InRoom,
        machine_cursor::transition::StageZero<
            crate::states::in_room::ToClient,
            close::Method,
            role::Server,
            in_memory_transport::Connection<&'static str>,
        >,
    >,
    eventual_processor_transition: machine_cursor::EventualTransitionRequest<
        impl Future<
            Output = Result<
                ProcessorTransition<
                    processor::StageOne<
                        crate::states::in_room::InRoom,
                        crate::states::in_room::from_client::Method,
                        role::Server,
                        in_memory_transport::Connection<&'static str>,
                    >,
                >,
                machine_cursor::processor::MultipleRequestsError<
                    <in_memory_transport::Connection<&'static str> as rpc::transport::Client>::Error,
                    <in_room::FromClientLoopbackHandler as rpc::Handler<
                        crate::states::in_room::from_client::Method,
                        loopback::Method,
                    >>::Error,
                    <in_room::FromClientHandler as rpc::Handler<
                        crate::states::in_room::from_client::Method,
                        crate::states::in_room::from_client::Method,
                    >>::Error,
                >,
            >,
        >,
    >,
) -> anyhow::Result<
    rpc::MachineCursor<Entrypoint, in_memory_transport::Connection<&'static str>, role::Server>,
> {
    let tiebreak_result = tiebreak::between_potential_processor_and_known_requester_transition(
        eventual_processor_transition,
        receiver,
    )
    .await?;
    match tiebreak_result {
        tiebreak::TiebreakResult::ProcessorWon(finalize_processor_transition) => {
            let (res, finalize) = finalize_processor_transition.extract_res();
            Ok(finalize
                .finish(res.try_extract_wrapper().ok().unwrap())
                .await?)
        }
        tiebreak::TiebreakResult::RequesterWon(finalize_requester_transition) => {
            let (res, finalize) = finalize_requester_transition.extract_res();
            Ok(finalize
                .finish(res.try_extract_wrapper().ok().unwrap())
                .await?)
        }
    }
}

async fn handle_processor_transition<
    TransitionMethod: rpc::Method<IsLeaf = is_leaf::True, CanTransition = can_transition::True>,
>(
    processor_transition_result: Result<
        ProcessorTransitionServerEntrypoint<
            crate::states::in_room::InRoom,
            in_memory_transport::Connection<&'static str>,
        >,
        machine_cursor::processor::MultipleRequestsError<std::io::Error, Infallible, Infallible>,
    >,
    mut handler: in_room::FromClientHandler,
    fused_receiver: futures::future::Fuse<
        tokio::sync::oneshot::Receiver<
            RequesterTransitionServerEntrypoint<
                crate::states::in_room::InRoom,
                TransitionMethod,
                in_memory_transport::Connection<&'static str>,
            >,
        >,
    >,
) -> anyhow::Result<
    rpc::MachineCursor<Entrypoint, in_memory_transport::Connection<&'static str>, role::Server>,
>
where
    <TransitionMethod as rpc::Method>::Res: rpc::RpcMessage + rpc::state::Has<Entrypoint>,
    crate::states::in_room::ToClient: rpc::method::FromDescendant<TransitionMethod>,
{
    let processor_transition = processor_transition_result?;
    // either they called a close or a leave, either way we remove them
    // from the room
    if let Some(removed_user) = handler.removed_user.take() {
        let (res, processor_transition) = processor_transition
            .next_with_requester(removed_user.requester)
            .await?
            .extract_res();

        Ok(processor_transition.finish(res.try_extract_wrapper().ok().unwrap()))
    } else {
        let receiver = fused_receiver.await.unwrap();
        match tiebreak::between_processor_and_requester_transition(processor_transition, receiver)
            .await
            .map_err(|e| anyhow!("{e:?}"))?
        {
            tiebreak::TiebreakResult::ProcessorWon(finalize_processor_transition) => {
                let (res, finalize) = finalize_processor_transition.extract_res();
                Ok(finalize
                    .finish(res.try_extract_wrapper().ok().unwrap())
                    .await?)
            }
            tiebreak::TiebreakResult::RequesterWon(finalize_requester_transition) => {
                let (res, finalize) = finalize_requester_transition.extract_res();
                Ok(finalize
                    .finish(res.try_extract_wrapper().ok().unwrap())
                    .await?)
            }
        }
    }
}

pub async fn server(transport: in_memory_transport::MemoryTransport<&'static str>) {
    let rooms: Rooms = Rooms::default();
    loop {
        let Ok(incoming) = transport.accept().await;
        let conn = incoming.accept().await.unwrap();
        let rooms = rooms.clone();

        warn!("value");

        tokio::spawn(
            async move {
                if let Err(err) = handle_incoming(conn, rooms.clone()).await {
                    warn!("error while handling client: {err}");
                };
            }
            .instrument(span::Span::current()),
        );
    }
}

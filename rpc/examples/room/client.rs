use futures::future::select;
use rpc::{
    MachineCursor, RpcMessage,
    in_memory_transport::{self, Connection},
    machine_cursor::{
        MachineCursorClient,
        transition::{requester::RequesterTransitionClientEntrypoint, tiebreak},
    },
    method::{can_transition, is_leaf, not_applicable},
    state::{self, Has, role},
};
use tracing::debug;

use crate::states::{
    self,
    entrypoint::join_room,
    in_room::{self, InRoom, ToClient},
};

pub mod handler;

pub async fn join_room(
    username: &'static str,
    room_id: &'static str,
    network: in_memory_transport::Network<&'static str>,
) -> anyhow::Result<MachineCursorClient<InRoom, in_memory_transport::Connection<&'static str>>> {
    let transport = network.new_transport(username);
    let cursor = MachineCursorClient::<crate::states::Entrypoint, _>::new(
        transport.connect(&"server").await.unwrap(),
    );
    let mut handler = not_applicable::Handler;
    let (processor, requester) = cursor.into_children_with_handler(&mut handler);
    let (res, requester_transition) = requester
        .request_transition::<join_room::JoinRoom>(join_room::Req {
            room_id: room_id.try_into().unwrap(),
            username: username.try_into().unwrap(),
        })
        .next()
        .await?
        .next()
        .await?
        .assert_need_processor()
        .extract_res();
    let res = res?;
    println!(
        "{} joined; existing participants: {:?}",
        username,
        res.0.inner()
    );
    let in_room = requester_transition.finish(processor, res.1).await?;
    Ok(in_room)
}

pub type JoinHandleResult<TransitionMethod> = RequesterTransitionClientEntrypoint<
    in_room::InRoom,
    TransitionMethod,
    Connection<&'static str>,
>;

pub async fn run_in_room<
    TransitionMethod: rpc::Method<IsLeaf = is_leaf::True, CanTransition = can_transition::True>,
>(
    join_handle: tokio::task::JoinHandle<Result<JoinHandleResult<TransitionMethod>, anyhow::Error>>,
    client_processor: rpc::machine_cursor::Processor<
        '_,
        InRoom,
        role::Client,
        ToClient,
        in_memory_transport::Connection<&'static str>,
        handler::ClientHandler,
    >,
    loopback_handler: handler::ClientHandler,
) -> anyhow::Result<
    MachineCursor<states::Entrypoint, in_memory_transport::Connection<&'static str>, role::Client>,
>
where
    <TransitionMethod as rpc::Method>::Res: RpcMessage + state::Has<states::Entrypoint>,
    states::in_room::from_client::Method: rpc::method::FromDescendant<TransitionMethod>,
{
    let processor_transition =
        client_processor.handle_requests::<in_room::Notify, _>(loopback_handler);
    let selection = select(join_handle, processor_transition).await;
    let tiebreak_result = match selection {
        futures::future::Either::Left((requester_transition, potential_processor_transition)) => {
            tiebreak::between_potential_processor_and_known_requester_transition(
                potential_processor_transition,
                requester_transition.unwrap()?,
            )
            .await?
        }
        futures::future::Either::Right((processor_transition, requester_transition_jh)) => {
            tiebreak::between_processor_and_requester_transition(
                processor_transition?,
                requester_transition_jh.await.unwrap()?,
            )
            .await
            .unwrap()
        }
    };

    let entrypoint_cursor: MachineCursorClient<states::Entrypoint, _> = match tiebreak_result {
        tiebreak::TiebreakResult::ProcessorWon(finalize_processor_transition) => {
            let (res, finalize_processor_transition) = finalize_processor_transition.extract_res();
            match res {
                in_room::ToClientRes::Notify(_) => unreachable!(),
                in_room::ToClientRes::Close(res) => {
                    debug!("close response received");
                    finalize_processor_transition.finish(res).await?
                }
            }
        }
        tiebreak::TiebreakResult::RequesterWon(finalize_requester_transition) => {
            let (res, finalize_requester_transition) = finalize_requester_transition.extract_res();
            finalize_requester_transition
                .finish(res.try_extract_wrapper().ok().unwrap())
                .await?
        }
    };

    Ok(entrypoint_cursor)
}

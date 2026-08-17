use futures::future::select;
use rpc::{
    MachineCursor, Transport, in_memory_transport,
    machine_cursor::{
        MachineCursorClient,
        transition::{RequestTransition, requester::RequesterTransition, tiebreak},
    },
    method::not_applicable,
    state::role,
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

#[allow(clippy::type_complexity)]
pub async fn run_in_room(
    join_handle: tokio::task::JoinHandle<
        Result<
            RequesterTransition<
                in_room::InRoom,
                RequestTransition<
                    in_room::from_client::Method,
                    in_room::from_client::leave::Method,
                    role::Client,
                    in_memory_transport::Connection<&'static str>,
                >,
            >,
            anyhow::Error,
        >,
    >,
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
> {
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
        futures::future::Either::Right((processor_transition, requester_transition)) => {
            tiebreak::between_processor_and_requester_transition(
                processor_transition?,
                requester_transition.await.unwrap()?,
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
            finalize_requester_transition.finish(res).await?
        }
    };

    Ok(entrypoint_cursor)
}

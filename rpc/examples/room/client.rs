use std::convert::Infallible;

use futures::future::select;
use rpc::{
    MachineCursor, ReqOf, Transport, in_memory_transport,
    machine_cursor::{
        MachineCursorClient,
        transition::{RequestTransition, requester::RequesterTransition, tiebreak},
    },
    method::{Ancestor, not_applicable},
    state::role,
};
use tracing::debug;

use crate::{
    Username,
    states::{
        self,
        entrypoint::join_room,
        in_room::{self, InRoom, ToClient, ToClientRes},
    },
};

#[derive(Clone)]
pub struct ClientHandler {
    username: Username,
}

impl ClientHandler {
    pub fn new(username: Username) -> Self {
        Self { username }
    }
}

impl<RM: Ancestor<in_room::Notify>> rpc::Handler<RM, in_room::Notify> for ClientHandler {
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RM, in_room::Notify>>(
        &mut self,
        replier: Replier,
        value: ReqOf<in_room::Notify>,
    ) -> rpc::traits::HandlerResult<RM, in_room::Notify, Replier, Self::Error> {
        debug!("{} handling notification", self.username);
        match value {
            in_room::Notification::Join(joiner) => {
                println!("{}: {joiner} joined", self.username)
            }
            in_room::Notification::Left(leaver) => println!("{}: {leaver} left", self.username),
            in_room::Notification::Mesg(message) => println!("{}: {message}", self.username),
        }
        replier.reply(()).await
    }
}

impl<RM: Ancestor<in_room::close::Method>> rpc::Handler<RM, in_room::close::Method>
    for ClientHandler
{
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RM, in_room::close::Method>>(
        &mut self,
        replier: Replier,
        (): ReqOf<in_room::close::Method>,
    ) -> rpc::traits::HandlerResult<RM, in_room::close::Method, Replier, Self::Error> {
        let wrapper = replier.new_wrapper();
        replier.reply(wrapper).await
    }
}

impl<RM: Ancestor<ToClient> + Ancestor<in_room::Notify> + Ancestor<in_room::close::Method>>
    rpc::Handler<RM, ToClient> for ClientHandler
{
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RM, ToClient>>(
        &mut self,
        replier: Replier,
        value: ReqOf<ToClient>,
    ) -> rpc::traits::HandlerResult<RM, ToClient, Replier, Self::Error> {
        match value {
            in_room::ToClientReq::Notif(notif) => {
                replier
                    .reply_with::<in_room::Notify, _>(self, notif, |v| {
                        in_room::ToClientRes::Notif(v)
                    })
                    .await
            }
            in_room::ToClientReq::Close(close_req) => {
                replier
                    .reply_with::<in_room::close::Method, _>(self, close_req, |v| {
                        ToClientRes::Close(v)
                    })
                    .await
            }
        }
    }
}

pub async fn join_room(
    username: &'static str,
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
            room_id: "room_id".try_into().unwrap(),
            username: username.try_into().unwrap(),
        })
        .next()
        .await?
        .try_into_need_processor()
        .ok()
        .unwrap()
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
    jh: tokio::task::JoinHandle<
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
        ClientHandler,
    >,
    loopback_handler: ClientHandler,
) -> anyhow::Result<
    MachineCursor<states::Entrypoint, in_memory_transport::Connection<&'static str>, role::Client>,
> {
    let processor_transition =
        client_processor.handle_requests::<in_room::Notify, _>(loopback_handler);
    let selection = select(jh, processor_transition).await;
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
                in_room::ToClientRes::Notif(_) => unreachable!(),
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

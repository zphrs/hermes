use std::convert::Infallible;

use rpc::{
    ReqOf,
    method::{Ancestor, ancestors},
};
use tracing::debug;

use crate::{
    Username,
    states::in_room::{self, ToClient, ToClientRes},
};

#[derive(Clone)]
pub struct ClientHandler {
    pub(crate) username: Username,
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

impl<RM: ancestors::Three<ToClient, in_room::Notify, in_room::close::Method>>
    rpc::Handler<RM, ToClient> for ClientHandler
{
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RM, ToClient>>(
        &mut self,
        replier: Replier,
        value: ReqOf<ToClient>,
    ) -> rpc::traits::HandlerResult<RM, ToClient, Replier, Self::Error> {
        match value {
            in_room::ToClientReq::Notify(notif) => {
                replier
                    .reply_with::<in_room::Notify, _>(self, notif, |v| {
                        in_room::ToClientRes::Notify(v)
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

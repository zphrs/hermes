mod max_len_str;

use futures::future::join;
use max_len_str::MaxLenStr;

use rpc::in_memory_transport::Network;
use tracing::{Instrument, debug, info_span};

use crate::{
    client::{ClientHandler, run_in_room},
    states::in_room::from_client::{leave, post},
};

pub type Username = MaxLenStr<256>;

pub type RoomId = MaxLenStr<256>;

pub mod states;

pub mod server;

pub mod client;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let network = Network::new();

    let server = network.new_transport("server");

    let server_jh = tokio::spawn(server::server(server).instrument(info_span!("server")));

    let span = info_span!("clients");
    let clients_fut = async move {
        let client1_in_room = client::join_room("client1", network.clone()).await?;
        let client2_in_room = client::join_room("client2", network).await?;

        debug!("logged in");
        let mut client1_handler = ClientHandler::new("client1".try_into().unwrap());
        let mut client2_handler = ClientHandler::new("client2".try_into().unwrap());
        let client1_handler_cloned = client1_handler.clone();
        let (client1_processor, client1_requester) =
            client1_in_room.into_children_with_handler(&mut client1_handler);
        let client2_handler_cloned = client2_handler.clone();
        let (client2_processor, client2_requester) =
            client2_in_room.into_children_with_handler(&mut client2_handler);

        let jh1 = tokio::spawn(
            async move {
                client1_requester
                    .request_loopback::<post::Method>("Hello, World!".try_into().unwrap())
                    .await?;
                debug!("sent hello world");

                let transition_request = client1_requester.request_transition::<leave::Method>(());

                anyhow::Ok(transition_request)
            }
            .instrument(info_span!("client1")),
        );
        let jh2 = tokio::spawn(
            async move {
                client2_requester
                    .request_loopback::<post::Method>("Hello, World!".try_into().unwrap())
                    .await?;
                debug!("sent hello world");
                let transition_request = client2_requester.request_transition::<leave::Method>(());

                anyhow::Ok(transition_request)
            }
            .instrument(info_span!("client2")),
        );
        let (res1, res2) = join(
            run_in_room(jh1, client1_processor, client1_handler_cloned.clone()),
            run_in_room(jh2, client2_processor, client2_handler_cloned),
        )
        .await;

        let _client1_entrypoint_mc = res1?;
        let _client2_entrypoint_mc = res2?;

        anyhow::Ok(())
    }
    .instrument(span);

    clients_fut.await?;

    server_jh.abort();

    anyhow::Ok(())
}

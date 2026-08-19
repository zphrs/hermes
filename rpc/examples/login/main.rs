//! Allows client to transition from Entrypoint to LoggedIn and back to Entrypoint.
//! All runs on one machine with the in_memory_transport.

use std::sync::Arc;

use rpc::{
    in_memory_transport::{ConnPair, Network, setup_conn},
    machine_cursor::MachineCursorClient,
    method::not_applicable::{self},
    state::{self},
};
use tracing::{Instrument, info_span};

use crate::states::{
    entrypoint::{self, login},
    logged_in::{self, LoggedIn},
};

mod server;
mod states;

pub(crate) async fn client<
    R: rpc::transport::Connection + Clone + Send + std::marker::Sync + 'static,
>(
    conn: R,
) -> anyhow::Result<()>
where
    <R as rpc::transport::Caller>::Error:
        Send + Sync + std::fmt::Debug + std::fmt::Display + 'static,
    <R as rpc::Caller>::OpenStreamFut: Send,
    <R as rpc::transport::BiStream>::SendStream: Send,
    <R as rpc::transport::BiStream>::RecvStream: Send,
{
    let entrypoint_cursor = MachineCursorClient::<states::Entrypoint, R>::new(conn);

    let mut handler = not_applicable::Handler;

    // try to login with incorrect_username, password
    // will result in a UserNotFoundError
    let (processor, requester) = entrypoint_cursor.into_children_with_handler(&mut handler);
    let (res, requester_transition) = login(requester, "incorrect_username", "password").await?;
    let res = res.expect_err("incorrect_username should result in a UserNotFound response");
    assert!(matches!(
        res,
        states::entrypoint::login::Error::UserNotFound(_)
    ));
    let entrypoint_cursor = requester_transition
        .finish(processor, res.extract_wrapper())
        .await?;

    // try to login with "admin", "incorrect_password"
    // will result in a PasswordIncorrect error
    let (processor, requester) = entrypoint_cursor.into_children_with_handler(&mut handler);
    let (res, requester_transition) = login(requester, "admin", "incorrect_password").await?;

    let res = res.expect_err("incorrect_password should result in a PasswordIncorrect response");

    assert!(matches!(res, login::Error::PasswordIncorrect(_)));

    let entrypoint_cursor = requester_transition
        .finish(processor, res.extract_wrapper())
        .await?;
    // try to login with "admin", "password"
    // will result in a successful login
    let (processor, requester) = entrypoint_cursor.into_children_with_handler(&mut handler);
    let (res, requester_transition) = login(requester, "admin", "password").await?;
    let res = res.expect("login should succeed");
    let logged_in_cursor = requester_transition.finish(processor, res).await?;
    // login succeeded
    let (processor, requester) = logged_in_cursor.into_children_with_handler(&mut handler);

    let requester_arc = Arc::new(requester);

    let requester = requester_arc.clone();

    let jh1 = tokio::spawn(async move {
        requester
            .request_loopback::<states::logged_in::ping::Method>(())
            .await
    });

    let requester = requester_arc.clone();
    let jh2 = tokio::spawn(async move {
        requester
            .request_loopback::<states::logged_in::ping::Method>(())
            .await
    });

    jh1.await.unwrap()?;
    jh2.await.unwrap()?;

    let (res, transition) = Arc::try_unwrap(requester_arc)
        .ok()
        .expect(
            "since req1 and req2 are awaited above, there is only the requester_arc reference left",
        )
        .request_transition::<logged_in::logout::Method>(())
        .next()
        .await?
        .assert_need_processor()
        .extract_res();
    // log out successful
    let entrypoint_cursor = transition.finish(processor, res);
    drop(entrypoint_cursor);
    Ok(())
}

async fn login<R: rpc::transport::Connection + Clone>(
    requester: rpc::machine_cursor::Requester<
        states::Entrypoint,
        rpc::state::role::Client,
        entrypoint::login::Method,
        R,
    >,
    username: &str,
    password: &str,
) -> anyhow::Result<(
    Result<state::Wrapper<LoggedIn>, login::Error>,
    rpc::machine_cursor::transition::requester::RequesterTransition<
        states::Entrypoint,
        rpc::machine_cursor::transition::requester::NeedProcessor<(), rpc::state::role::Client, R>,
    >,
)>
where
    <R as rpc::transport::Caller>::Error:
        Send + Sync + std::fmt::Debug + std::fmt::Display + 'static,
{
    let (res, requester_transition) = requester
        .request_transition::<states::entrypoint::login::Method>(states::entrypoint::login::Req {
            username: username.try_into().unwrap(),
            password: password.try_into().unwrap(),
        })
        .next()
        .await?
        .assert_need_processor()
        .extract_res();
    Ok((res.into_inner(), requester_transition))
}

#[tokio::main]
async fn main() {
    use tracing_subscriber;

    tracing_subscriber::fmt::init();
    let net = Network::new();
    let ConnPair {
        server_conn,
        client_conn,
    } = setup_conn(0, 1, &net).await;

    let server_jh = tokio::spawn(server::server(server_conn).instrument(info_span!("server")));

    let client_jh = tokio::spawn(client(client_conn).instrument(info_span!("client")));

    let res = client_jh.await.unwrap();
    if let Err(err) = res {
        eprintln!("{}\n{}", err, err.backtrace());
        panic!()
    }

    server_jh
        .await
        .unwrap()
        .expect_err("server should have errored out");
}

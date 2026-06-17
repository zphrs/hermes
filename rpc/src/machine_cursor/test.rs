use std::time::Duration;

use futures::{TryStreamExt, stream::FuturesUnordered};
use tokio::task::JoinSet;
use tracing::warn;

use crate::{
    MachineCursor, Transport,
    traits::{method::not_applicable, state},
    transport::Incoming,
};

mod login {
    use maxlen::MaxLen;

    use super::actions;
    use std::convert::Infallible;

    use crate::traits::{
        method::{can_transition, not_applicable},
        state::{self, Wrapper},
    };

    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    #[cbor(flat)]
    pub enum Response {
        #[n(0)]
        TryAgain(#[n(0)] state::Wrapper<State>),
        #[n(1)]
        Ok(#[n(0)] state::Wrapper<actions::State>),
    }

    impl crate::Method for Method {
        /// whether or not to let the login go through
        type Req = bool;
        type Res = Response;
        type CanTransition = can_transition::True;
    }
    pub struct Method;

    pub struct State;

    impl crate::traits::State for State {
        /// we use [`None`] as a method that is impossible to construct
        type ClientMethod = not_applicable::Method;

        type ServerMethod = Method;
    }

    impl crate::Handler for Method {
        type Error = Infallible;

        async fn handle<Replier: crate::transport::ReplyHelper<Self>>(
            &mut self,
            replier: Replier,
            value: <Self as crate::Method>::Req,
        ) -> Result<Replier::Receipt<Self>, crate::traits::HandlerError<Replier::Error, Self::Error>>
        {
            match value {
                true => replier.reply(Response::Ok(Wrapper::new())).await,
                false => replier.reply(Response::TryAgain(Wrapper::new())).await,
            }
        }
    }
}

mod actions {

    use std::convert::Infallible;

    use maxlen::MaxLen;

    use crate::traits::{method::can_transition, state};

    use super::login;

    #[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    #[cbor(flat)]
    pub enum Request {
        #[n(0)]
        Logout(),
        #[n(1)]
        Ping(),
    }

    impl TryFrom<Request> for PingRequest {
        type Error = Request;

        fn try_from(value: Request) -> Result<Self, Self::Error> {
            tracing::warn!("trying from {value:?}");
            match value {
                Request::Ping() => Ok(PingRequest),
                other => Err(other),
            }
        }
    }

    impl From<PingRequest> for Request {
        fn from(_value: PingRequest) -> Self {
            Request::Ping()
        }
    }

    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct PingRequest;

    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct PingResponse;

    pub struct PingMethod;

    impl crate::Method for PingMethod {
        type Req = PingRequest;

        type Res = PingResponse;

        type CanTransition = can_transition::False;
    }

    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    #[cbor(flat)]
    pub enum Response {
        #[n(0)]
        Logout(#[cbor(skip)] state::Wrapper<login::State>),
        #[n(1)]
        Ping(#[cbor(skip)] state::Wrapper<State>),
    }

    impl From<PingResponse> for Response {
        fn from(_value: PingResponse) -> Self {
            Response::Ping(state::Wrapper::new())
        }
    }
    #[derive(Clone)]
    pub struct Method;
    impl crate::Method for Method {
        type Req = Request;

        type Res = Response;

        type CanTransition = can_transition::True;
    }

    impl crate::Handler<PingMethod> for Method {
        type Error = Infallible;

        async fn handle<Replier: crate::transport::ReplyHelper<PingMethod>>(
            &mut self,
            replier: Replier,
            _value: <PingMethod as crate::Method>::Req,
        ) -> Result<
            Replier::Receipt<PingMethod>,
            crate::traits::HandlerError<Replier::Error, Self::Error>,
        > {
            replier.reply(PingResponse).await
        }
    }

    impl crate::Handler for Method {
        type Error = Infallible;

        async fn handle<Replier: crate::transport::ReplyHelper<Self>>(
            &mut self,
            replier: Replier,
            value: <Self as crate::Method>::Req,
        ) -> Result<Replier::Receipt<Self>, crate::traits::HandlerError<Replier::Error, Self::Error>>
        {
            match value {
                Request::Logout() => replier.reply(Response::Logout(state::Wrapper::new())).await,
                Request::Ping() => {
                    replier
                        .reply_with::<PingMethod, _>(self, PingRequest, From::from)
                        .await
                }
            }
        }
    }
    pub struct State;

    impl crate::traits::State for State {
        type ClientMethod = Method;

        type ServerMethod = Method;
    }
}

#[tokio::test]
#[test_log::test]
async fn test_state_flow() {
    let network = crate::in_memory_transport::Network::new();

    let mut js = JoinSet::new();
    static SERVER_ADDR: u32 = 0;
    // server
    {
        let net = network.clone();
        js.spawn(async move {
            let tp = net.new_transport(SERVER_ADDR);
            let incoming = tp.accept().await.expect("infallible");
            let conn = incoming.accept().await.expect("successful incoming");
            let mut login_cursor =
                MachineCursor::<login::State, _, _>::new(conn, state::role::Server);

            let actions_cursor = loop {
                let (mut handler, sender) = login_cursor.into_parts(login::Method);
                let res = handler.handle_transition_request().await.unwrap();
                let (res, split_receipt) =
                    MachineCursor::<login::State, _, state::role::Server>::split_transition_receipt(
                        res, sender,
                    ).await.unwrap();

                match res {
                    login::Response::TryAgain(wrapper) => {
                        login_cursor = MachineCursor::from_split_receipt(split_receipt, wrapper)
                            .await
                            .unwrap()
                    }
                    login::Response::Ok(wrapper) => {
                        break MachineCursor::from_split_receipt(split_receipt, wrapper)
                            .await
                            .unwrap();
                    }
                }
            };

            let (mut handler, sender) = actions_cursor.into_parts(actions::Method);
            let client_jh = tokio::spawn(async move {
                let js = FuturesUnordered::new();
                for _ in 0..10 {
                    js.push(sender.request_loopback::<actions::PingMethod>(actions::PingRequest));
                }
                js.try_collect::<Vec<_>>().await.unwrap();
                sender
                    .request_transition::<actions::Method>(actions::Request::Logout())
                    .await
                    .unwrap()
            });
            let (res, transition_receipt) = tokio::select! {
                _finalized_handler = handler.handle_requests::<actions::PingMethod, _>(actions::Method) => {
                    panic!("client shouldn't log out for this example")
                },
                client_jh = client_jh => {
                    client_jh.unwrap().extract_result()
                }
            };
            let wrapper = match res {
                actions::Response::Logout(wrapper) => wrapper,
                actions::Response::Ping(_wrapper) => unreachable!("we made a logout request above"),
            };

            let _login_cursor =
                MachineCursor::<actions::State, _, state::role::Server>::from_transition_receipt(
                    transition_receipt,
                    wrapper,
                    handler,
                ).await.unwrap();
        });
    };
    // client
    {
        let net = network.clone();
        js.spawn(async move {
            let tp = net.new_transport(1u32);
            let conn = tp.connect(&SERVER_ADDR).await.unwrap();
            let login_cursor = MachineCursor::<login::State, _, _>::new(conn, state::role::Client);
            let (handler, sender) = login_cursor.into_parts(not_applicable::Handler);
            // request login to fail
            let (res, receipt) = sender
                .request_transition::<login::Method>(false)
                .await
                .unwrap()
                .extract_result();
            let login_cursor = match res {
                login::Response::TryAgain(wrapper) => {
                    MachineCursor::<login::State, _, state::role::Client>::from_transition_receipt(
                        receipt, wrapper, handler,
                    )
                    .await
                    .unwrap()
                }
                login::Response::Ok(_wrapper) => {
                    unreachable!("we asked to be rejected")
                }
            };
            let (handler, sender) = login_cursor.into_parts(not_applicable::Handler);
            let (res, receipt) = sender
                .request_transition::<login::Method>(true)
                .await
                .unwrap()
                .extract_result();

            let actions_cursor = match res {
                login::Response::TryAgain(_wrapper) => {
                    unreachable!("we asked to be accepted")
                }
                login::Response::Ok(wrapper) => {
                    MachineCursor::<login::State, _, state::role::Client>::from_transition_receipt(
                        receipt, wrapper, handler,
                    )
                    .await
                    .unwrap()
                }
            };
            let (mut handler, sender) = actions_cursor.into_parts(actions::Method);
            warn!(
                "should flesh out how the functions work when both the client and the server
                are both listening and replying to one another."
            );

            let loopback_futs =
                tokio::spawn(async move {
                    FuturesUnordered::from_iter((0..10).map(|_| {
                        sender.request_loopback::<actions::PingMethod>(actions::PingRequest)
                    }))
                    .try_collect::<Vec<_>>()
                    .await
                    .unwrap();
                    sender
                });

            let pending_transition_receipt = handler
                .handle_requests::<actions::PingMethod, _>(actions::Method)
                .await
                .unwrap();

            let sender = loopback_futs.await.expect("loopback futs not to panic");

            let (res, split_receipt) =
                MachineCursor::<actions::State, _, state::role::Client>::split_transition_receipt(
                    pending_transition_receipt,
                    sender,
                )
                .await
                .unwrap();

            match res {
                actions::Response::Logout(wrapper) => {
                    let _new_machine = MachineCursor::from_split_receipt(split_receipt, wrapper)
                        .await
                        .unwrap();
                    tokio::time::sleep(Duration::from_millis(1)).await;
                    warn!("got here!");
                }
                actions::Response::Ping(wrapper) => {
                    MachineCursor::from_split_receipt(split_receipt, wrapper)
                        .await
                        .unwrap();
                    panic!("ping should have been a loopback request")
                }
            };
        });
    }
    js.join_all().await;
}

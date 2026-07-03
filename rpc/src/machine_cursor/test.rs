use std::{
    marker::PhantomData,
    pin::pin,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use futures::{FutureExt, TryStreamExt, poll, select, stream::FuturesUnordered};
use tokio::task::JoinSet;
use tracing::warn;

use crate::{
    MachineCursor, Transport,
    in_memory_transport::Connection,
    machine_cursor::processor::DelayedReplier,
    in_memory_transport::{Connection, SendStream},
    machine_cursor::{
        MachineCursorClient, MachineCursorServer,
        sender::RequestTransition,
        state_handler::{DelayedReplier, PendingTransitionReceipt},
        test::waitlist::TableOffer,
        tiebreak,
    },
    state::{priority::Server, role},
    traits::{method::not_applicable, state},
    transport::{Client, Incoming},
};

mod login {
    use maxlen::MaxLen;

    use super::actions;
    use std::convert::Infallible;

    use crate::{
        state::priority::server_wins,
        traits::{
            Prioritized,
            method::{can_transition, not_applicable},
            state::{self, Wrapper},
        },
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

    crate::define_prioritized!(State, server_wins);

    impl crate::Handler for Method {
        type Error = Infallible;

        async fn handle<Replier: crate::transport::ReplyHelper<Self>>(
            &mut self,
            replier: Replier,
            value: <Self as crate::Method>::Req,
        ) -> Result<Replier::Receipt<Self>, crate::traits::HandleError<Replier::Error, Self::Error>>
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

    use crate::traits::{Prioritized, method::can_transition, state};

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
            crate::traits::HandleError<Replier::Error, Self::Error>,
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
        ) -> Result<Replier::Receipt<Self>, crate::traits::HandleError<Replier::Error, Self::Error>>
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

    impl Prioritized for State {
        type Priority = bool;

        fn client_priority(
            _request: &<Self::ClientMethod as crate::Method>::Req,
        ) -> Self::Priority {
            false
        }

        fn server_priority(
            _request: &<Self::ServerMethod as crate::Method>::Req,
        ) -> Self::Priority {
            true
        }
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
            let mut stream = conn.accept_stream().await.unwrap();
            let receipt = conn
                .handle_one_request_with_handler(
                    DelayedReplier::new(),
                    &mut stream.1,
                    &mut login::Method,
                )
                .await
                .unwrap();
            let mut login_cursor =
                MachineCursorServer::<login::State, _>::new(conn);

            let actions_cursor = loop {
                let (mut handler, sender) = login_cursor.into_parts(login::Method);
                let stream = handler.accept_stream().await.unwrap();
                let res = handler.handle_transition_request(stream).await.unwrap();
                let (res, split_receipt) = res.split(sender).await.unwrap();

                match res {
                    login::Response::TryAgain(wrapper) => {
                        login_cursor = MachineCursor::from_split_receipt(split_receipt, wrapper, handler)
                            .await
                            .unwrap()
                    }
                    login::Response::Ok(wrapper) => {
                        break MachineCursor::from_split_receipt(split_receipt, wrapper, handler)
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
            let login_cursor = MachineCursorClient::<login::State, _>::new(conn);
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

            let (res, split_receipt) = pending_transition_receipt.split(sender).await.unwrap();

            match res {
                actions::Response::Logout(wrapper) => {
                    let _new_machine =
                        MachineCursor::from_split_receipt(split_receipt, wrapper, handler)
                            .await
                            .unwrap();
                    tokio::time::sleep(Duration::from_millis(1)).await;
                    warn!("got here!");
                }
                actions::Response::Ping(wrapper) => {
                    MachineCursor::from_split_receipt(split_receipt, wrapper, handler)
                        .await
                        .unwrap();
                    panic!("ping should have been a loopback request")
                }
            };
        });
    }
    js.join_all().await;
}
mod waitlist {
    //! a trivial example of a restaurant waiting list, where a customer (the
    //! client) can enter via the [`HostStand`] [`State`] and request a
    //! transition to the [`WaitingList`] [`State`]. From there, the server
    //! might transition the customer to the [`Seated`] [`State`] via the
    //! [`TableOffer`] [`Method`] that will transition the
    //! [`Connection`](crate::transport::Connection) to the [`Seated`] state.
    //! Alternatively in the [`Waitlist`] [`State`], the client might leave the
    //! waiting list, transitioning back to the [`HostStand`] state.
    //!

    use std::convert::Infallible;

    use crate::{
        Handler, Method, State, define_prioritized,
        state::{self, Prioritized, priority::server_wins},
        traits::method::{can_transition, not_applicable::NotApplicable},
    };

    pub struct HostStand;

    impl crate::State for HostStand {
        type ClientMethod = NotApplicable;

        type ServerMethod = Join;
    }

    define_prioritized!(HostStand, server_wins);

    pub struct Join;

    impl crate::Method for Join {
        type Req = ();

        type Res = state::Wrapper<WaitingList>;

        type CanTransition = can_transition::True;
    }

    impl Handler for Join {
        type Error = Infallible;

        async fn handle<Replier: crate::transport::ReplyHelper<Self>>(
            &mut self,
            replier: Replier,
            _value: <Self as Method>::Req,
        ) -> Result<
            <Replier as crate::transport::ReplyHelper<Self>>::Receipt<Self>,
            crate::traits::HandleError<
                <Replier as crate::transport::ReplyHelper<Self>>::Error,
                <Self as Handler<Self>>::Error,
            >,
        > {
            replier.reply(state::Wrapper::new()).await
        }
    }

    pub struct WaitingList;

    impl crate::State for WaitingList {
        type ClientMethod = TableOffer;

        type ServerMethod = Leave;
    }

    define_prioritized!(WaitingList, server_wins);

    pub struct TableOffer;

    impl crate::Method for TableOffer {
        type Req = ();

        type Res = state::Wrapper<Seated>;

        type CanTransition = can_transition::True;
    }

    pub struct Leave;

    impl crate::Method for Leave {
        type Req = ();

        type Res = state::Wrapper<HostStand>;

        type CanTransition = can_transition::True;
    }

    impl crate::Handler for Leave {
        type Error = Infallible;

        async fn handle<Replier: crate::transport::ReplyHelper<Self>>(
            &mut self,
            replier: Replier,
            _value: <Self as Method>::Req,
        ) -> Result<
            <Replier as crate::transport::ReplyHelper<Self>>::Receipt<Self>,
            crate::traits::HandleError<
                <Replier as crate::transport::ReplyHelper<Self>>::Error,
                <Self as Handler<Self>>::Error,
            >,
        > {
            replier.reply(state::Wrapper::new()).await
        }
    }

    pub struct Seated;

    impl crate::State for Seated {
        type ClientMethod = NotApplicable;

        type ServerMethod = NotApplicable;
    }
}

#[tokio::test]
#[test_log::test]
async fn tiebreak() {
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
            let mut host_stand_cursor = MachineCursorServer::<waitlist::HostStand, _>::new(conn);
            let seated_cursor = loop {
                let (mut handler, sender) = host_stand_cursor.into_parts(waitlist::Join);

                let stream = handler.accept_stream().await.unwrap();
                // wait for client to join waiting list
                let transition = handler.handle_transition_request(stream).await.unwrap();

                let (res, receipt) = transition.split(sender).await.unwrap();
                // create waiting_list cursor after client joined waiting list
                let waiting_list = MachineCursor::from_split_receipt(receipt, res, handler)
                    .await
                    .unwrap();
                let (mut handler, sender) = waiting_list.into_parts(waitlist::Leave);

                let wait_for_available_table = async {
                    // wait 10ms (pretend arbitrary delay before table becomes
                    // available) to take client off of waiting list
                    tokio::time::sleep(Duration::from_millis(10)).await;

                    sender.request_transition::<TableOffer>(())
                };

                let request_transition_jh = tokio::spawn(wait_for_available_table);
                let abort_handle = request_transition_jh.abort_handle();
                let mut request_transition_jh = request_transition_jh.fuse();

                let received_logout_request = Arc::new(AtomicBool::new(false));
                let fut = async {
                    let stream = handler.accept_stream().await.unwrap();
                    received_logout_request.store(true, std::sync::atomic::Ordering::AcqRel);
                    handler.handle_transition_request(stream).await
                }
                .fuse();
                let mut pending_transition_receipt_fut = Box::pin(fut);

                enum Res<'a> {
                    RequestTransition(
                        RequestTransition<(), TableOffer, role::Server, Connection<u32>>,
                    ),
                    TransitionReceipt(
                        PendingTransitionReceipt<
                            'a,
                            waitlist::WaitingList,
                            waitlist::Leave,
                            role::Server,
                            Connection<u32>,
                            (bool, PhantomData<waitlist::WaitingList>),
                            SendStream,
                        >,
                    ),
                }
                let result = select! {
                    request_transition = request_transition_jh => {
                        Res::RequestTransition(request_transition.unwrap())
                    },
                    transition_receipt = pending_transition_receipt_fut => {
                        Res::TransitionReceipt(transition_receipt.unwrap())
                    }
                };

                match result {
                    Res::RequestTransition(request_transition) => {
                        // if we got here, then we sent off a request to transition
                        // before we fully read in a request to abort.

                        if received_logout_request.load(std::sync::atomic::Ordering::AcqRel) {
                            let pending_transition_receipt =
                                pending_transition_receipt_fut.as_mut().await.unwrap();

                            let tiebreak = MachineCursorServer::tiebreak(
                                pending_transition_receipt,
                                request_transition,
                            )
                            .await
                            .unwrap();

                            drop(pending_transition_receipt_fut);

                            let (res, parts) = tiebreak.into_parts(handler).await.unwrap();

                            match res {
                                tiebreak::Res::Remote(seated) => {
                                    break MachineCursorServer::from_tiebreak_parts(seated, parts);
                                }
                                tiebreak::Res::Local(host_stand) => {
                                    host_stand_cursor =
                                        MachineCursorServer::from_tiebreak_parts(host_stand, parts);
                                }
                            }
                        } else {
                            // if we're here then we don't have any pending
                            // requests that we're handling so we must race
                            // between the request_transition and
                            // the pending_transition_receipt.
                            //
                            // If the remote has sent off a logout request
                            // then the remote won't resolve our request
                            // without tiebreaking. Either the remote tiebreaks
                            // in our direction and sends back an acknowledgement
                            // of our request before we get their initial
                            // request or we get their request.
                            let mut request_transition = request_transition.fuse();
                            let res = select! {
                                transition_receipt = request_transition => {
                                    transition_receipt.unwrap()
                                }
                            };
                        }
                    }
                    Res::TransitionReceipt(pending_transition_receipt) => {
                        let request_transition = match poll!(request_transition_jh) {
                            std::task::Poll::Ready(ready) => ready.unwrap(),
                            std::task::Poll::Pending => {
                                abort_handle.abort(); // still was sleeping
                                todo!()
                            }
                        };

                        MachineCursorServer::tiebreak(
                            pending_transition_receipt,
                            request_transition,
                        )
                        .await
                        .unwrap();
                        todo!()
                    }
                };
            };

            // let request_to_transition = request_transition_jh.await.unwrap();

            // MachineCursorServer::tiebreak(pending_transition_receipt, request_to_transition);
        });
    };
    // client
    {
        let net = network.clone();
        js.spawn(async move {
            let tp = net.new_transport(1u32);
            let conn = tp.connect(&SERVER_ADDR).await.unwrap();
            let login_cursor = MachineCursorClient::<waitlist::HostStand, _>::new(conn);
        });
    }
    js.join_all().await;
}

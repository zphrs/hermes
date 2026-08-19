pub mod final_endpoint;
pub mod from_processor_to_completion;
mod fuzz_tiebreak;

mod test_states;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

pub mod prelude {
    pub use super::test_states::{
        Entrypoint, Request,
        client_endpoint::ClientEndpoint,
        entrypoint::{client, server},
        server_endpoint::ServerEndpoint,
    };
}

use futures::{FutureExt as _, select};
use tokio::task::JoinSet;
use tracing::{Instrument, Span, debug, info_span};

use crate::{
    in_memory_transport::{ConnPair, Connection, setup_conn},
    machine_cursor::{
        MachineCursorClient, MachineCursorServer,
        test::waitlist::{TableOffer, WaitingList},
        transition::{
            self, StageZero, processor::ProcessorTransition, requester::RequesterTransition,
            tiebreak,
        },
    },
    state::role,
    traits::method::not_applicable::{self},
    transport::Incoming,
};

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
        Handler, Method, define_prioritized,
        method::Ancestor,
        state::{self, priority::server_wins},
        traits::method::{can_transition, is_leaf, not_applicable::NotApplicable},
    };

    pub struct HostStand;

    impl crate::State for HostStand {
        type ClientHandles = NotApplicable;

        type ServerHandles = Join;
    }

    define_prioritized!(HostStand, server_wins);

    pub struct Join;

    impl crate::Method for Join {
        type Req = ();

        type Res = state::Wrapper<WaitingList>;

        type CanTransition = can_transition::True;
        type IsLeaf = is_leaf::True;
    }

    impl<RootMethod: Ancestor<Join>> Handler<RootMethod> for Join {
        type Error = Infallible;

        fn handle<Replier: crate::ReplyHelper<RootMethod, Self>>(
            &mut self,
            replier: Replier,
            (): crate::ReqOf<Self>,
        ) -> impl Future<Output = crate::traits::HandlerResult<RootMethod, Self, Replier, Self::Error>>
        {
            let res = replier.new_wrapper();
            replier.reply(res)
        }
    }

    pub struct WaitingList;

    impl crate::State for WaitingList {
        type ClientHandles = TableOffer;

        type ServerHandles = Leave;
    }

    define_prioritized!(WaitingList, server_wins);

    pub struct TableOffer;

    impl crate::Method for TableOffer {
        type Req = ();

        type Res = state::Wrapper<Seated>;

        type CanTransition = can_transition::True;
        type IsLeaf = is_leaf::True;
    }

    impl crate::Handler<TableOffer> for TableOffer {
        type Error = Infallible;

        fn handle<Replier: crate::transport::ReplyHelper<Self, TableOffer>>(
            &mut self,
            replier: Replier,
            _value: <Self as Method>::Req,
        ) -> impl Future<
            Output = Result<
                <Replier as crate::transport::ReplyHelper<Self, TableOffer>>::Receipt<Self>,
                crate::traits::HandleError<
                    <Replier as crate::transport::ReplyHelper<Self, TableOffer>>::Error,
                    <Self as Handler<TableOffer, Self>>::Error,
                >,
            >,
        > {
            let res = replier.new_wrapper();
            replier.reply(res)
        }
    }

    pub struct Leave;

    impl crate::Method for Leave {
        type Req = ();

        type Res = state::Wrapper<HostStand>;

        type CanTransition = can_transition::True;
        type IsLeaf = is_leaf::True;
    }

    impl crate::Handler<Leave> for Leave {
        type Error = Infallible;

        fn handle<Replier: crate::transport::ReplyHelper<Self, Leave>>(
            &mut self,
            replier: Replier,
            _value: <Self as Method>::Req,
        ) -> impl Future<
            Output = Result<
                <Replier as crate::transport::ReplyHelper<Self, Leave>>::Receipt<Self>,
                crate::traits::HandleError<
                    <Replier as crate::transport::ReplyHelper<Self, Leave>>::Error,
                    <Self as Handler<Leave, Self>>::Error,
                >,
            >,
        > {
            let res = replier.new_wrapper();
            replier.reply(res)
        }
    }

    pub struct Seated;

    impl crate::State for Seated {
        type ClientHandles = NotApplicable;

        type ServerHandles = NotApplicable;
    }
}

#[tokio::test]
#[test_log::test]
async fn join() {
    debug!("Here!");
    let network = crate::in_memory_transport::Network::new();

    let mut js = JoinSet::new();
    const SERVER_ADDR: u8 = 0;

    let ConnPair {
        client_conn,
        server_conn,
    } = setup_conn(SERVER_ADDR, 1, &network).await;
    // server

    js.spawn(async move {
        let host_stand_cursor = MachineCursorServer::<waitlist::HostStand, _>::new(server_conn);

        let mut join_handler = waitlist::Join;

        let (processor, requester) =
            host_stand_cursor.into_children_with_handler(&mut join_handler);
        // wait for client to join waiting list
        let transition = processor
            .handle_transition_request()
            .await?
            .next_with_requester(requester)
            .await?;
        let (res, transition) = transition.extract_res();
        let _waiting_list = transition.finish(res);

        debug!("server transitioned to waitlist");
        anyhow::Ok(())
    });

    // client
    js.spawn(async move {
        let host_stand_cursor = MachineCursorClient::<waitlist::HostStand, _>::new(client_conn);
        let waiting_list_cursor = {
            let mut handler = not_applicable::Handler;
            let (processor, requester) = host_stand_cursor.into_children_with_handler(&mut handler);
            let (res, requester_transition) = requester
                // prime a request to join the list
                .request_transition::<waitlist::Join>(())
                // actually commit to sending the transition request
                // (would be a problem if our processor ever started handling requests)
                .next()
                .await?
                .next()
                .await?
                // asserts that the remote didn't send off a transition request
                // (in this case impossible since Method is NotApplicable)
                .assert_need_processor()
                // takes Res out of the NeedProcessor type
                .extract_res();
            requester_transition.finish(processor, res).await.unwrap()
        };

        let mut handler = waitlist::TableOffer;

        let (_processor, _requester) = waiting_list_cursor.into_children_with_handler(&mut handler);
        anyhow::Ok(())
    });

    for res in js.join_all().await {
        res.unwrap();
    }
}

#[tokio::test]
#[test_log::test]
async fn test_tiebreak() {
    debug!("Here!");
    let network = crate::in_memory_transport::Network::new();

    let mut js = JoinSet::new();
    const SERVER_ADDR: u32 = 0;

    let net = network.clone();
    let server_fut = async move {
        let tp = net.new_transport(SERVER_ADDR);
        let incoming = tp.accept().await.expect("infallible");
        let conn = incoming.accept().await.expect("successful incoming");
        let mut host_stand_cursor = MachineCursorServer::<waitlist::HostStand, _>::new(conn);
        let _seated_cursor = loop {
            debug!("looping");
            let mut handler = waitlist::Join;
            let (processor, requester) = host_stand_cursor.into_children_with_handler(&mut handler);
            // wait for client to join waiting list
            let transition = processor.handle_transition_request();
            let transition = transition.await.unwrap();
            let transition = transition.next_with_requester(requester).await.unwrap();
            let (res, transition) = transition.extract_res();
            let waiting_list = transition.finish(res);

            debug!("transitioned to waitlist");
            let mut handler = waitlist::Leave;
            let (processor, requester) = waiting_list.into_children_with_handler(&mut handler);

            let wrapped_requester = Arc::new(Mutex::new(Some(requester)));

            let weak_wrapped_requester = Arc::downgrade(&wrapped_requester);
            let table_ready = {
                async move {
                    // wait 10ms (pretend arbitrary delay before table becomes
                    // available) to take client off of waiting list

                    tokio::time::sleep(Duration::from_millis(2)).await;
                    let res = Some(
                        weak_wrapped_requester
                            .upgrade()?
                            .lock()
                            .unwrap()
                            .take()?
                            .request_transition::<waitlist::TableOffer>(()),
                    );
                    debug!("sent off transition");

                    res
                }
                .instrument(Span::current())
            };

            let request_transition_jh = tokio::spawn(table_ready);
            let request_transition_abort_handle = request_transition_jh.abort_handle();
            let mut request_transition_jh = request_transition_jh.fuse();

            let mut processor_transition_fut = processor.handle_transition_request();

            enum Select {
                Processor(
                    ProcessorTransition<
                        transition::processor::StageOne<
                            waitlist::WaitingList,
                            <WaitingList as crate::State>::ServerHandles,
                            role::Server,
                            Connection<u32>,
                        >,
                    >,
                ),
                Requester(
                    #[allow(clippy::type_complexity)]
                    Option<
                        RequesterTransition<
                            waitlist::WaitingList,
                            StageZero<
                                waitlist::TableOffer,
                                waitlist::TableOffer,
                                role::Server,
                                Connection<u32>,
                            >,
                        >,
                    >,
                ),
            }

            // race between the transition and table_ready
            let select = select! {
                transition = processor_transition_fut => {
                    Select::Processor(transition.unwrap())
                }
                requester = request_transition_jh => {
                    Select::Requester(requester.unwrap())
                }
            };

            let tiebreak_res = match select {
                Select::Processor(processor_transition) => {
                    let maybe_requester = wrapped_requester.lock().unwrap().take();
                    if let Some(requester) = maybe_requester {
                        request_transition_abort_handle.abort();
                        let res = processor_transition
                            .next_with_requester(requester)
                            .await
                            .unwrap();
                        let (wrapper, transition) = res.extract_res();
                        host_stand_cursor = transition.finish(wrapper);
                        continue;
                    }
                    // if above was not some then it must have gotten to the
                    // transition
                    let requester_transition = request_transition_jh.await.unwrap().unwrap();
                    debug!("waiting for an ack on its request");

                    transition::tiebreak::between_processor_and_requester_transition(
                        processor_transition,
                        requester_transition,
                    )
                    .await
                    .unwrap()
                }
                Select::Requester(maybe_requester) => {
                    // should always be some here because this thread doesn't take
                    // until the processor wins
                    let requester_transition = maybe_requester.unwrap();
                    tiebreak::between_potential_processor_and_known_requester_transition(
                        processor_transition_fut,
                        requester_transition,
                    )
                    .await
                    .unwrap()
                }
            };

            match tiebreak_res {
                transition::tiebreak::TiebreakResult::ProcessorWon(
                    finalize_processor_transition,
                ) => {
                    let (res, finalize_processor_transition) =
                        finalize_processor_transition.extract_res();
                    host_stand_cursor = finalize_processor_transition.finish(res).await.unwrap();
                    continue;
                }
                transition::tiebreak::TiebreakResult::RequesterWon(
                    finalize_requester_transition,
                ) => {
                    let (res, finalize_requester_transition) =
                        finalize_requester_transition.extract_res();
                    let seated_cursor = finalize_requester_transition.finish(res).await.unwrap();
                    break seated_cursor;
                }
            }
        };
    };
    js.spawn(server_fut.instrument(info_span!("server")));

    // client

    let net = network.clone();
    let client_fut = async move {
        let tp = net.new_transport(1u32);
        let conn = tp.connect(&SERVER_ADDR).await.unwrap();
        let mut host_stand_cursor = MachineCursorClient::<waitlist::HostStand, _>::new(conn);
        let _seated_cursor = loop {
            debug!("looping");
            let waiting_list_cursor = {
                // join the list
                let mut handler = not_applicable::Handler;
                let (processor, requester) =
                    host_stand_cursor.into_children_with_handler(&mut handler);
                let requester_transition = requester.request_transition::<waitlist::Join>(());
                let requester_transition = requester_transition.next().await.unwrap();

                let transition::requester::Need::Processor(requester_transition) =
                    requester_transition.next().await.unwrap()
                else {
                    panic!("unexpected Need variant")
                };
                let (res, requester_transition) = requester_transition.extract_res();
                requester_transition.finish(processor, res).await.unwrap()
            };
            debug!("joined waitlist");

            const SHOULD_LEAVE: bool = false;
            let mut handler = TableOffer;
            let (processor, requester) =
                waiting_list_cursor.into_children_with_handler(&mut handler);

            if SHOULD_LEAVE {
                tokio::time::sleep(Duration::from_millis(4)).await;

                let requester_transition = requester.request_transition::<waitlist::Leave>(());

                tracing::trace!("sending leave request; waiting for tiebreak");
                let processor_transition = processor.handle_transition_request();
                match tiebreak::between_potential_processor_and_known_requester_transition(
                    processor_transition,
                    requester_transition,
                )
                .await
                .unwrap()
                {
                    transition::tiebreak::TiebreakResult::ProcessorWon(
                        finalize_processor_transition,
                    ) => {
                        let (res, finalize_processor_transition) =
                            finalize_processor_transition.extract_res();
                        debug!("processor won");
                        let seated_cursor =
                            finalize_processor_transition.finish(res).await.unwrap();
                        debug!("processor finished");

                        break seated_cursor;
                    }
                    transition::tiebreak::TiebreakResult::RequesterWon(
                        finalize_requester_transition,
                    ) => {
                        let (res, finalize_requester_transition) =
                            finalize_requester_transition.extract_res();
                        debug!("requester won");
                        host_stand_cursor =
                            finalize_requester_transition.finish(res).await.unwrap();
                        debug!("requester finished");

                        continue;
                    }
                }
            } else {
                let processor_transition = processor.handle_transition_request();
                let (res, processor_transition) = processor_transition
                    .await
                    .unwrap()
                    .next_with_requester(requester)
                    .await
                    .unwrap()
                    .extract_res();
                let seated_cursor = processor_transition.finish(res);
                break seated_cursor;
            }
        };
    };
    js.spawn(client_fut.instrument(info_span!("client")));

    js.join_all().await;
}

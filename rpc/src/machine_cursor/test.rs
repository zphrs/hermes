pub mod final_endpoint;
pub mod from_processor_to_completion;
mod setup_conn;
mod tiebreak;
pub use setup_conn::{ConnPair, setup_conn};
pub(self) mod test_states;

use std::{
    pin::pin,
    sync::{Arc, Mutex},
    time::Duration,
};

pub mod prelude {
    pub use super::{
        ConnPair, setup_conn,
        test_states::{
            Entrypoint, Request,
            client_endpoint::ClientEndpoint,
            entrypoint::{client, server},
            server_endpoint::ServerEndpoint,
        },
    };
}

use futures::{FutureExt as _, select};
use tokio::task::JoinSet;
use tracing::{Instrument, Span, debug, info_span};

use crate::{
    Transport,
    in_memory_transport::Connection,
    machine_cursor::{
        MachineCursorClient, MachineCursorServer,
        test::waitlist::{TableOffer, WaitingList},
        transition::{
            self, RequestTransition, processor::ProcessorTransition,
            requester::RequesterTransition, tiebreak::from_processor_to_completion,
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
        type ClientMethod = NotApplicable;

        type ServerMethod = Join;
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

        fn handle<Replier: crate::transport::ReplyHelper<Self, RootMethod>>(
            &mut self,
            replier: Replier,
            value: <Self as Method>::Req,
        ) -> impl Future<
            Output = Result<
                <Replier as crate::transport::ReplyHelper<Self, RootMethod>>::Receipt<Self>,
                crate::traits::HandleError<
                    <Replier as crate::transport::ReplyHelper<Self, RootMethod>>::Error,
                    <Self as Handler<RootMethod, Self>>::Error,
                >,
            >,
        > {
            replier.reply(state::Wrapper::new())
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
        type IsLeaf = is_leaf::True;
    }

    impl crate::Handler<TableOffer> for TableOffer {
        type Error = Infallible;

        fn handle<Replier: crate::transport::ReplyHelper<Self, TableOffer>>(
            &mut self,
            replier: Replier,
            value: <Self as Method>::Req,
        ) -> impl Future<
            Output = Result<
                <Replier as crate::transport::ReplyHelper<Self, TableOffer>>::Receipt<Self>,
                crate::traits::HandleError<
                    <Replier as crate::transport::ReplyHelper<Self, TableOffer>>::Error,
                    <Self as Handler<TableOffer, Self>>::Error,
                >,
            >,
        > {
            replier.reply(state::Wrapper::new())
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
            value: <Self as Method>::Req,
        ) -> impl Future<
            Output = Result<
                <Replier as crate::transport::ReplyHelper<Self, Leave>>::Receipt<Self>,
                crate::traits::HandleError<
                    <Replier as crate::transport::ReplyHelper<Self, Leave>>::Error,
                    <Self as Handler<Leave, Self>>::Error,
                >,
            >,
        > {
            replier.reply(state::Wrapper::new())
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
async fn join() {
    debug!("Here!");
    let network = crate::in_memory_transport::Network::new();

    let mut js = JoinSet::new();
    const SERVER_ADDR: u32 = 0;
    // server
    {
        let net = network.clone();
        js.spawn(async move {
            let tp = net.new_transport(SERVER_ADDR);
            let incoming = tp.accept().await.expect("infallible");
            let conn = incoming.accept().await.expect("successful incoming");
            let host_stand_cursor = MachineCursorServer::<waitlist::HostStand, _>::new(conn);

            let (processor, requester) = host_stand_cursor.into_parts(waitlist::Join);
            // wait for client to join waiting list
            let transition = processor.handle_transition_request().1.await.unwrap();
            let transition = transition.next_with_requester(requester).await.unwrap();
            let (res, transition) = transition.extract_res();
            let waiting_list = transition.finish(res);

            debug!("server transitioned to waitlist");
        });
    };
    // client

    let net = network.clone();
    const SHOULD_LEAVE: bool = false;
    js.spawn(async move {
        let tp = net.new_transport(1u32);
        let conn = tp.connect(&SERVER_ADDR).await.unwrap();
        let host_stand_cursor = MachineCursorClient::<waitlist::HostStand, _>::new(conn);

        debug!("client looping");
        let waiting_list_cursor = {
            // join the list
            let (processor, requester) = host_stand_cursor.into_parts(not_applicable::Handler);
            let requester_transition = requester.request_transition::<waitlist::Join>(());
            let requester_transition = requester_transition.next().await.unwrap();

            let transition::requester::Need::Processor(requester_transition) = requester_transition
            else {
                panic!("unexpected Need variant")
            };
            let (res, requester_transition) = requester_transition.extract_res();
            requester_transition.finish(processor, res).await.unwrap()
        };

        let (processor, requester) = waiting_list_cursor.into_parts(waitlist::TableOffer);
    });

    js.join_all().await;
}

#[tokio::test]
#[test_log::test]
async fn tiebreak() {
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
            let (processor, requester) = host_stand_cursor.into_parts(waitlist::Join);
            // wait for client to join waiting list
            let transition = processor.handle_transition_request().1.await.unwrap();
            let transition = transition.next_with_requester(requester).await.unwrap();
            let (res, transition) = transition.extract_res();
            let waiting_list = transition.finish(res);

            debug!("transitioned to waitlist");

            let (processor, requester) = waiting_list.into_parts(waitlist::Leave);

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

            let (to_sacrifice, processor_transition_fut) = processor.handle_transition_request();

            let processor_transition_fut = processor_transition_fut.fuse();
            let mut processor_transition_fut = pin!(processor_transition_fut);

            enum Select {
                Processor(
                    ProcessorTransition<
                        transition::processor::Entrypoint<
                            waitlist::WaitingList,
                            <WaitingList as crate::State>::ServerMethod,
                            role::Server,
                            Connection<u32>,
                        >,
                    >,
                ),
                Requester(
                    Option<
                        RequesterTransition<
                            waitlist::WaitingList,
                            RequestTransition<
                                (),
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

                    transition::tiebreak::tiebreak(
                        processor_transition,
                        requester_transition,
                        to_sacrifice,
                    )
                    .await
                }
                Select::Requester(maybe_requester) => {
                    // should always be some here because this thread doesn't take
                    // until the processor wins
                    let requester_transition = maybe_requester.unwrap();
                    from_processor_to_completion(
                        processor_transition_fut,
                        to_sacrifice,
                        requester_transition,
                    )
                    .await
                }
            };

            match tiebreak_res.unwrap() {
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
        let seated_cursor = loop {
            debug!("looping");
            let waiting_list_cursor = {
                // join the list
                let (processor, requester) = host_stand_cursor.into_parts(not_applicable::Handler);
                let requester_transition = requester.request_transition::<waitlist::Join>(());
                let requester_transition = requester_transition.next().await.unwrap();

                let transition::requester::Need::Processor(requester_transition) =
                    requester_transition
                else {
                    panic!("unexpected Need variant")
                };
                let (res, requester_transition) = requester_transition.extract_res();
                requester_transition.finish(processor, res).await.unwrap()
            };
            debug!("joined waitlist");

            const SHOULD_LEAVE: bool = false;

            let (processor, requester) = waiting_list_cursor.into_parts(TableOffer);

            if SHOULD_LEAVE {
                tokio::time::sleep(Duration::from_millis(4)).await;

                let requester_transition = requester.request_transition::<waitlist::Leave>(());

                tracing::trace!("sending leave request; waiting for tiebreak");
                let (to_sacrifice, processor_transition) = processor.handle_transition_request();
                match from_processor_to_completion(
                    processor_transition,
                    to_sacrifice,
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
                let (_to_sacrifice, processor_transition) = processor.handle_transition_request();
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

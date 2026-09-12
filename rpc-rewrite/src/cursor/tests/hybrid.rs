pub mod states {

    pub mod entrypoint {
        use std::time::Duration;

        use crate::{
            cursor::{self, state},
            marker::{False, NotApplicable, True, not_applicable},
            method::{
                Descendant, LeafHandler, ReqOf, ResOf, TransitionBranchHandler,
                TransitionLeafHandler,
            },
        };

        pub struct State;
        impl cursor::State for State {
            type ClientHandles = NotApplicable;

            type ServerHandles = RootMethod;
        }

        impl state::Entrypoint for State {}

        pub struct RootMethod;

        impl TransitionBranchHandler for RootMethod {
            type NextHandler = Self;

            async fn handle_transition<
                'a,
                TR: crate::method::replier::transition::Replier<Self>,
            >(
                self,
                request: ReqOf<'a, Self>,
                replier: TR,
            ) -> crate::method::handler::transition::HandleTransitionResult<
                'a,
                Self::NextHandler,
                TR,
                Self,
            > {
                let res = match request {
                    RootRequest::Ping(request) => {
                        replier.reply_with_leaf(request, &mut Ping).await?
                    }
                    RootRequest::Transition(request) => {
                        let (out, _) = replier.transition_with_leaf(request, Transition).await?;
                        out
                    }
                    RootRequest::Sleep(request) => {
                        replier.reply_with_leaf(request, &mut Sleep).await?
                    }
                };

                Ok((res, self))
            }
        }

        #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
        pub enum RootRequest<'buf> {
            #[n(0)]
            Ping(#[n(0)] &'buf minicbor::bytes::ByteSlice),
            #[n(1)]
            Transition(#[n(0)] ReqOf<'buf, Transition>),
            #[n(2)]
            Sleep(#[n(0)] ReqOf<'buf, Sleep>),
        }

        #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
        pub enum RootResponse<'buf> {
            #[n(0)]
            Ping(#[n(0)] &'buf minicbor::bytes::ByteSlice),
            #[n(1)]
            Transition(#[n(0)] ResOf<'buf, Transition>),
            #[n(2)]
            Sleep(#[n(0)] ResOf<'buf, Sleep>),
        }

        pub struct Ping;

        impl crate::Method for Ping {
            type Req<'buf> = &'buf minicbor::bytes::ByteSlice;
            type Res<'buf> = &'buf minicbor::bytes::ByteSlice;

            type Transitions = False;
            type HasDescendants = False;
        }

        impl LeafHandler for Ping {
            async fn handle<'a>(&mut self, request: ReqOf<'a, Self>) -> ResOf<'a, Self> {
                request
            }
        }

        pub struct Transition;

        impl TransitionLeafHandler for Transition {
            type NextHandler = not_applicable::Handler;

            async fn handle_transition<'a>(
                &mut self,
                (): ReqOf<'a, Self>,
                wrapper_credit: state::WrapperCredit<Self>,
            ) -> (ResOf<'a, Self>, Self::NextHandler) {
                (wrapper_credit.into(), not_applicable::Handler)
            }
        }

        impl crate::Method for Transition {
            type Req<'buf> = ();
            type Res<'buf> = state::Wrapper<NotApplicable>;

            type Transitions = True;
            type HasDescendants = False;
        }

        pub struct Sleep;

        impl crate::Method for Sleep {
            type Req<'buf> = Duration;

            type Res<'buf> = ();

            type Transitions = False;

            type HasDescendants = False;
        }

        impl LeafHandler for Sleep {
            async fn handle<'a>(&mut self, request: ReqOf<'a, Self>) -> ResOf<'a, Self> {
                tokio::time::sleep(request).await;
            }
        }

        impl Descendant<RootMethod> for Ping {
            fn req_to_parent<'buf>(
                req: crate::method::ReqOf<'buf, Self>,
            ) -> crate::method::ReqOf<'buf, RootMethod> {
                RootRequest::Ping(req)
            }

            fn res_to_parent<'buf>(
                res: crate::method::ResOf<'buf, Self>,
            ) -> crate::method::ResOf<'buf, RootMethod> {
                RootResponse::Ping(res)
            }
        }

        impl Descendant<RootMethod> for Transition {
            fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, RootMethod> {
                RootRequest::Transition(req)
            }

            fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, RootMethod> {
                RootResponse::Transition(res)
            }
        }

        impl Descendant<RootMethod> for Sleep {
            fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, RootMethod> {
                RootRequest::Sleep(req)
            }

            fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, RootMethod> {
                RootResponse::Sleep(res)
            }
        }

        impl crate::Method for RootMethod {
            type Req<'buf> = RootRequest<'buf>;

            type Res<'buf> = RootResponse<'buf>;

            type Transitions = True;

            type HasDescendants = True;
        }
    }
}
use std::net::SocketAddr;

use crate::{
    cursor::Cursor,
    marker::{self, not_applicable},
};

use states::entrypoint;

async fn server(endpoint: quinn::Endpoint) -> anyhow::Result<()> {
    let connection = super::accept_client(&endpoint).await?;
    let a_cursor = Cursor::<entrypoint::State, marker::Server, _>::new(connection);
    let (processor, requester) = a_cursor.into_processor_and_requester(entrypoint::RootMethod);
    let mut buf = Vec::new();
    // we expect an error out here
    let (res, _next_handler, cursor_credit) = processor
        .handle_transition_request(&mut buf, requester)
        .await?;

    let wrapper = match res {
        entrypoint::RootResponse::Transition(wrapper) => wrapper,
        _ => unimplemented!(),
    };

    let b_cursor = Cursor::from_cursor_credit(cursor_credit, wrapper);

    b_cursor.wait_to_close().await?;

    Ok(())
}

async fn client(endpoint: quinn::Endpoint, server_addr: SocketAddr) -> anyhow::Result<()> {
    let connection = super::connect_to_server(&endpoint, server_addr).await?;
    let cursor = Cursor::<entrypoint::State, marker::Client, _>::new(connection);

    let (processor, requester) = cursor.into_processor_and_requester(not_applicable::Handler);
    let mut read_buf = Vec::new();
    let (res, cursor_credit) = requester
        .request_transition::<entrypoint::Transition>((), &mut read_buf, processor)
        .await?;

    let _b_cursor = Cursor::from_cursor_credit(cursor_credit, res);

    Ok(())
}

#[test_log::test]
fn immediate_transition() -> anyhow::Result<()> {
    super::harness(client, server)
}

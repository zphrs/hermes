

use std::{panic, time::Duration};

use tokio::task::JoinSet;
use tracing::{debug, warn};

use crate::{
    transport::{Client, Incoming}, Transport,
    in_memory_transport::{self, MemoryTransport},
    state_machine_transitions::{
        RootHandlerWrapper,
        tests::{
            actions::{LogoutRequest, PingRequest},
            authenticate::{LoginMethod, Responses},
        },
    },
};

mod authenticate {
    use maxlen::MaxLen;
    use std::convert::Infallible;

    use super::actions;
    use crate::{RootHandler, state_machine_transitions::RootHandlerWrapper};

    #[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct LoginRequest {
        #[n(0)]
        try_again: bool,
    }

    impl LoginRequest {
        pub fn new(try_again: bool) -> Self {
            Self { try_again }
        }
    }

    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub enum Responses {
        #[n(0)]
        Ok(#[cbor(skip)] RootHandlerWrapper<actions::RootRequest, actions::RootHandler>),
        #[n(1)]
        TryAgain(#[cbor(skip)] RootHandlerWrapper<LoginRequest, LoginMethod>),
    }

    impl std::fmt::Debug for Responses {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Ok(_arg0) => f.debug_tuple("Ok").field(&()).finish(),
                Self::TryAgain(_arg0) => f.debug_tuple("TryAgain").field(&()).finish(),
            }
        }
    }

    pub struct LoginMethod;

    impl RootHandler<LoginRequest> for LoginMethod {
        type Error = Infallible;
        type Response = Responses;

        async fn handle<
            T: futures_io::AsyncWrite + Unpin + Sync + Send,
            TransportError: Send,
        >(
            &mut self,
            root: LoginRequest,
            replier: crate::Replier<'_, T>,
        ) -> Result<
            crate::ReplyReceipt<Self::Response>,
            crate::ClientError<TransportError, Self::Error>,
        > {
            replier.reply_with::<_, _, LoginMethod>(self, root).await
        }
    }

    impl crate::Method for LoginMethod {
        type Req = LoginRequest;

        type Res = Responses;

        type Error = Infallible;
    }

    impl crate::Call for LoginMethod {
        async fn call(&mut self, value: Self::Req) -> Result<Self::Res, Self::Error> {
            if value.try_again {
                Ok(Responses::TryAgain(Default::default()))
            } else {
                Ok(Responses::Ok(Default::default()))
            }
        }
    }
}

mod actions {
    use std::convert::Infallible;

    use maxlen::MaxLen;

    use crate::{Call, state_machine_transitions::RootHandlerWrapper};

    #[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    #[cbor(flat)]
    pub enum RootRequest {
        #[n(0)]
        Loopback(#[n(0)] LoopbackRequest),
        #[n(1)]
        Logout(),
    }
    #[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    #[cbor(flat)]
    pub enum LoopbackRequest {
        #[n(0)]
        Ping(#[n(0)] PingRequest),
    }

    #[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    #[cbor(flat)]
    pub enum LoopbackResponse {
        #[n(0)]
        Ping(#[n(0)] PingResponse)
    }

    impl From<LoopbackResponse> for RootHandlerWrapper<RootRequest, RootHandler> {
        fn from(value: LoopbackResponse) -> Self {
            match value {
                LoopbackResponse::Ping(ping_response) => ping_response.0,
            }
        }
    }
    #[derive(Clone, Copy)]
    pub struct LoopbackHandler;

    impl crate::Method for LoopbackHandler {
        type Req = LoopbackRequest;

        type Res = LoopbackResponse;

        type Error = Infallible;
    }

    impl crate::Call for LoopbackHandler {
        async fn call(
            &mut self,
            value: Self::Req,
        ) -> Result<Self::Res, Self::Error> {
            match value {
                LoopbackRequest::Ping(ping_request) => PingMethod.call(ping_request).await.map(LoopbackResponse::Ping),
            }
        }
    }

    impl crate::RootHandler<LoopbackRequest> for LoopbackHandler {
        type Response = LoopbackResponse;

        type Error = Infallible;

        async fn handle<T: futures_io::AsyncWrite + Unpin + Sync + Send, TransportError: Send>(
            &mut self,
            root: LoopbackRequest,
            replier: crate::Replier<'_, T>,
        ) -> Result<crate::ReplyReceipt<Self::Response>, crate::ClientError<TransportError, Self::Error>> {
            let out = match root {
                LoopbackRequest::Ping(ping_request) => replier.reply_with(&mut PingMethod, ping_request).await,
            };
            out.map(|v| v.map(LoopbackResponse::Ping))
        }
    }

    pub struct RootHandler;

    impl crate::RootHandler<RootRequest> for RootHandler {
        type Error = Infallible;
        type Response = NextHandler;

        async fn handle<
            T: futures_io::AsyncWrite + Unpin + Sync + Send,
            TransportError: Send,
        >(
            &mut self,
            root: RootRequest,
            replier: crate::Replier<'_, T>,
        ) -> Result<
            crate::ReplyReceipt<Self::Response>,
            crate::ClientError<TransportError, Self::Error>,
        > {
            Ok(match root {
                RootRequest::Loopback(LoopbackRequest::Ping(ping)) => replier
                    .reply_with(&mut PingMethod, ping)
                    .await?
                    .map(|r| NextHandler::Actions(LoopbackResponse::Ping(r).into())),
                RootRequest::Logout() => replier
                    .reply::<_, _, LogoutMethod>(LogoutResponse::default())
                    .await?
                    .map(|r| NextHandler::Authenticate(r.0)),
            })
        }
    }
    #[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct PingRequest();

    impl From<PingRequest> for RootRequest {
        fn from(value: PingRequest) -> Self {
            Self::Loopback(LoopbackRequest::Ping(value))
        }
    }

    pub struct LogoutRequest();

    impl TryFrom<RootRequest> for LoopbackRequest {
        type Error = RootRequest;

        fn try_from(value: RootRequest) -> Result<Self, Self::Error> {
            match value {
                RootRequest::Loopback(loopback_request) => Ok(loopback_request),
                v => Err(v),
            }
        }
    }

    impl From<LoopbackRequest> for RootRequest {
        fn from(value: LoopbackRequest) -> Self {
            RootRequest::Loopback(value)
        }
    }

    impl From<LogoutRequest> for RootRequest {
        fn from(_value: LogoutRequest) -> Self {
            Self::Logout()
        }
    }

    #[derive(Debug)]
    pub struct LogoutMethod;

    impl crate::Method for LogoutMethod {
        type Req = LogoutRequest;

        type Res = LogoutResponse;

        type Error = Infallible;
    }

    #[derive(Debug)]
    pub struct PingMethod;

    impl crate::Method for PingMethod {
        type Req = PingRequest;

        type Res = PingResponse;

        type Error = Infallible;
    }

    impl Call for PingMethod {
        async fn call(&mut self, _value: Self::Req) -> Result<Self::Res, Self::Error> {
            Ok(PingResponse::default())
        }
    }

    #[derive(Default, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct PingResponse(#[cbor(skip)] pub RootHandlerWrapper<RootRequest, RootHandler>);

    impl std::fmt::Debug for PingResponse {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("PingResponse").field(&()).finish()
        }
    }

    #[derive(Default, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct LogoutResponse(
        #[cbor(skip)]
        pub  RootHandlerWrapper<
            super::authenticate::LoginRequest,
            super::authenticate::LoginMethod,
        >,
    );

    pub enum NextHandler {
        Actions(RootHandlerWrapper<RootRequest, RootHandler>),
        Authenticate(
            RootHandlerWrapper<
                super::authenticate::LoginRequest,
                super::authenticate::LoginMethod,
            >,
        ),
    }

    impl std::fmt::Debug for LogoutResponse {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("LogoutResponse").field(&()).finish()
        }
    }
}

#[tokio::test]
#[test_log::test]
async fn login_flow() {
    let net = in_memory_transport::Network::new();

    let net1 = net.clone();
    let tp = MemoryTransport::new(net1);
    let server_addr = tp.address();

    let ls = tokio::task::LocalSet::new();
    ls.run_until(async move {
        let mut js = JoinSet::new();
        // server
        js.spawn_local(async move {
            let mut login_wrapper: RootHandlerWrapper<authenticate::LoginRequest, LoginMethod> =
                RootHandlerWrapper::default();
            let incoming = tp.accept().await.unwrap();
            let conn = incoming.accept().await.unwrap();
            'outer: loop {
                let mut actions = loop {
                    let Ok(stream) = conn.accept_stream().await else {
                        debug!("breaking out of server loop due to accept_stream error");
                        break 'outer;
                    };
                    match login_wrapper
                        .handle_state_transition_request(&conn, stream, &mut LoginMethod)
                        .await
                        .unwrap()
                    {
                        authenticate::Responses::Ok(method_wrapper) => break method_wrapper,
                        authenticate::Responses::TryAgain(new_login_wrapper) => {
                            login_wrapper = new_login_wrapper
                        }
                    }
                };

                login_wrapper = loop {
                    let stream = match conn.accept_stream().await {
                        Ok(stream) => stream,
                        Err(e) => {
                            warn!("{e}");
                            break 'outer;
                        },
                    };
                    match actions
                        .handle_state_transition_request(&conn, stream, &mut actions::RootHandler)
                        .await
                        .unwrap()
                    {
                        actions::NextHandler::Actions(method_wrapper) => actions = method_wrapper,
                        actions::NextHandler::Authenticate(method_wrapper) => {
                            break method_wrapper
                        }
                    }
                }
            }
        });
        // client
        let _client_handle = js.spawn_local(async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let tp = MemoryTransport::new(net);
            let conn = tp.connect(&server_addr).await.unwrap();

            let login_wrapper: RootHandlerWrapper<
                authenticate::LoginRequest,
                authenticate::LoginMethod,
            > = Default::default();

            let res = login_wrapper
                .query::<LoginMethod, _>(authenticate::LoginRequest::new(true), &conn)
                .await
                .unwrap();

            let Responses::TryAgain(login_wrapper) = res else {
                panic!()
            };

            let Responses::Ok(actions_wrapper) = login_wrapper
                .query::<LoginMethod, _>(authenticate::LoginRequest::new(false), &conn)
                .await
                .unwrap()
            else {
                panic!()
            };

            let actions_wrapper = actions_wrapper
                .query::<actions::PingMethod, _>(PingRequest(), &conn)
                .await
                .unwrap()
                .0;

            let _login_wrapper = actions_wrapper
                .query::<actions::LogoutMethod, _>(LogoutRequest(), &conn)
                .await
                .unwrap()
                .0;
        });
        js.join_all().await;
    })
    .await;
}

#[tokio::test]
#[test_log::test]
async fn concurrent_after_login() {
    let net = in_memory_transport::Network::new();

    let net1 = net.clone();
    let tp = MemoryTransport::new(net1);
    let server_addr = tp.address();

    let ls = tokio::task::LocalSet::new();

    ls.run_until(async move {
        let mut js = JoinSet::new();
        // server
        js.spawn_local(async move {
            let mut login_wrapper: RootHandlerWrapper<authenticate::LoginRequest, LoginMethod> =
                RootHandlerWrapper::default();
            let incoming = tp.accept().await.unwrap();
            let conn = incoming.accept().await.unwrap();
            'outer: loop {
                let mut actions = loop {
                    let Ok(stream) = conn.accept_stream().await else {
                        debug!("breaking out of server loop due to accept_stream error");
                        break 'outer;
                    };
                    match login_wrapper
                        .handle_state_transition_request(&conn, stream, &mut LoginMethod)
                        .await
                        .unwrap()
                    {
                        authenticate::Responses::Ok(method_wrapper) => break method_wrapper,
                        authenticate::Responses::TryAgain(new_login_wrapper) => {
                            login_wrapper = new_login_wrapper
                        }
                    }
                };
                login_wrapper = loop {
                    let Ok(res) = actions
                        .handle_concurrent_requests(
                            &conn,
                            actions::LoopbackHandler,
                            &mut actions::RootHandler,
                        )
                        .await
                    else {
                        return;
                    };
                    match res {
                        actions::NextHandler::Actions(root_handler_wrapper) => {
                            actions = root_handler_wrapper
                        }
                        actions::NextHandler::Authenticate(root_handler_wrapper) => {
                            break root_handler_wrapper
                        }
                    };
                };
            }
        });
        // client
        let _client_handle = js.spawn_local(async move {
            let tp = MemoryTransport::new(net);
            let conn = tp.connect(&server_addr).await.unwrap();

            let login_wrapper: RootHandlerWrapper<
                authenticate::LoginRequest,
                authenticate::LoginMethod,
            > = Default::default();

            let res = login_wrapper
                .query::<LoginMethod, _>(authenticate::LoginRequest::new(true), &conn)
                .await
                .unwrap();

            let Responses::TryAgain(login_wrapper) = res else {
                panic!()
            };

            let Responses::Ok(actions_wrapper) = login_wrapper
                .query::<LoginMethod, _>(authenticate::LoginRequest::new(false), &conn)
                .await
                .unwrap()
            else {
                panic!()
            };

            let actions_wrapper = actions_wrapper
                .query::<actions::PingMethod, _>(PingRequest(), &conn)
                .await
                .unwrap()
                .0;

            let _login_wrapper = actions_wrapper
                .query::<actions::LogoutMethod, _>(LogoutRequest(), &conn)
                .await
                .unwrap()
                .0;
        });
        js.join_all().await;
    })
    .await;
}

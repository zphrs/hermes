use std::{panic, time::Duration};

use futures::{TryStreamExt, stream::FuturesUnordered};
use tokio::{task::JoinSet, time::Instant};
use tracing::{debug, warn};

use crate::{
    Transport,
    in_memory_transport::{self, MemoryTransport},
    state_machine_transitions::MethodWrapper,
    transport::{Client, Incoming},
};

mod authenticate {
    use maxlen::MaxLen;
    use std::convert::Infallible;

    use super::actions;
    use crate::state_machine_transitions::MethodWrapper;

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
        Ok(#[cbor(skip)] MethodWrapper<actions::RootHandler>),
        #[n(1)]
        TryAgain(#[cbor(skip)] MethodWrapper<LoginMethod>),
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

    impl crate::Method for LoginMethod {
        type Req = LoginRequest;

        type Res = Responses;

        type Error = Infallible;
    }

    impl crate::Call for LoginMethod {
        async fn call<T: futures::AsyncWrite + Unpin + Send + Sync, TransportError>(
            &mut self,
            replier: crate::Replier<'_, T, Self>,
            value: Self::Req,
        ) -> Result<
            crate::transport::ReplyReceipt<Self::Res>,
            crate::ClientError<TransportError, Self::Error>,
        > {
            if value.try_again {
                Ok(replier
                    .reply(Responses::TryAgain(Default::default()))
                    .await?)
            } else {
                Ok(replier.reply(Responses::Ok(Default::default())).await?)
            }
        }
    }
}

mod actions {
    use std::{convert::Infallible, time::Duration};

    use maxlen::MaxLen;
    use minicbor::CborLen;

    use crate::{Call, state_machine_transitions::MethodWrapper};

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
        #[n(1)]
        LongPing(#[n(0)] LongPingRequest),
    }

    #[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct LongPingRequest {
        #[n(0)]
        pub duration_millis: u64,
    }

    #[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    #[cbor(flat)]
    pub enum LoopbackResponse {
        #[n(0)]
        Ping(#[n(0)] PingResponse),
        #[n(1)]
        LongPing(#[n(0)] LongPingResponse),
    }
    #[derive(minicbor::Encode, Default, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct LongPingResponse(#[cbor(skip)] pub MethodWrapper<RootHandler>);

    impl From<LoopbackResponse> for MethodWrapper<RootHandler> {
        fn from(value: LoopbackResponse) -> Self {
            match value {
                LoopbackResponse::Ping(ping_response) => ping_response.0,
                LoopbackResponse::LongPing(long_ping_response) => long_ping_response.0,
            }
        }
    }

    pub struct LongPingMethod;

    impl crate::Method for LongPingMethod {
        type Req = LongPingRequest;

        type Res = LongPingResponse;

        type Error = Infallible;
    }

    impl crate::Call for LongPingMethod {
        async fn call<T: futures::AsyncWrite + Unpin + Send + Sync, TransportError>(
            &mut self,
            replier: crate::Replier<'_, T, Self>,
            value: Self::Req,
        ) -> Result<
            crate::transport::ReplyReceipt<Self::Res>,
            crate::ClientError<TransportError, Self::Error>,
        > {
            tokio::time::sleep(Duration::from_millis(value.duration_millis)).await;
            replier.reply(LongPingResponse::default()).await
        }
    }

    impl std::fmt::Debug for LongPingResponse {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("LongPingResponse").finish()
        }
    }

    #[derive(Clone, Copy)]
    pub struct LoopbackHandler;

    impl From<PingRequest> for LoopbackRequest {
        fn from(value: PingRequest) -> Self {
            Self::Ping(value)
        }
    }

    impl From<LongPingRequest> for LoopbackRequest {
        fn from(value: LongPingRequest) -> Self {
            Self::LongPing(value)
        }
    }

    impl crate::Method for LoopbackHandler {
        type Req = LoopbackRequest;

        type Res = LoopbackResponse;

        type Error = Infallible;
    }

    impl crate::Call for LoopbackHandler {
        async fn call<T: futures::AsyncWrite + Unpin + Send + Sync, TransportError>(
            &mut self,
            replier: crate::Replier<'_, T, Self>,
            value: Self::Req,
        ) -> Result<
            crate::transport::ReplyReceipt<Self::Res>,
            crate::ClientError<TransportError, Self::Error>,
        > {
            Ok(match value {
                LoopbackRequest::Ping(ping_request) => PingMethod
                    .call(replier.change_method(&ping_request), ping_request)
                    .await?
                    .map(LoopbackResponse::Ping),
                LoopbackRequest::LongPing(long_ping_request) => LongPingMethod
                    .call(replier.change_method(&long_ping_request), long_ping_request)
                    .await?
                    .map(LoopbackResponse::LongPing),
            })
        }
    }

    #[derive(Debug)]
    pub struct RootHandler;

    impl crate::Method for RootHandler {
        type Req = RootRequest;

        type Res = NextHandler;

        type Error = Infallible;
    }

    impl crate::Call for RootHandler {
        async fn call<T: futures::AsyncWrite + Unpin + Send + Sync, TransportError>(
            &mut self,
            replier: crate::Replier<'_, T, Self>,
            value: Self::Req,
        ) -> Result<
            crate::transport::ReplyReceipt<Self::Res>,
            crate::ClientError<TransportError, Self::Error>,
        > {
            Ok(match value {
                RootRequest::Loopback(loopback) => LoopbackHandler
                    .call(replier.change_method(&loopback), loopback)
                    .await?
                    .map(|v| NextHandler::Actions(v.into())),
                RootRequest::Logout() => LogoutMethod
                    .call(replier.change_method(&LogoutRequest()), LogoutRequest())
                    .await?
                    .map(|v| NextHandler::Authenticate(v.0)),
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

    pub struct LogoutRequest();

    impl From<LogoutRequest> for RootRequest {
        fn from(_value: LogoutRequest) -> Self {
            Self::Logout()
        }
    }

    impl From<LongPingRequest> for RootRequest {
        fn from(value: LongPingRequest) -> Self {
            Self::Loopback(value.into())
        }
    }

    #[derive(Debug)]
    pub struct LogoutMethod;

    impl crate::Method for LogoutMethod {
        type Req = LogoutRequest;

        type Res = LogoutResponse;

        type Error = Infallible;
    }

    impl crate::Call for LogoutMethod {
        async fn call<T: futures::AsyncWrite + Unpin + Send + Sync, TransportError>(
            &mut self,
            replier: crate::Replier<'_, T, Self>,
            _value: Self::Req,
        ) -> Result<
            crate::transport::ReplyReceipt<Self::Res>,
            crate::ClientError<TransportError, Self::Error>,
        > {
            replier.reply(LogoutResponse::default()).await
        }
    }

    #[derive(Debug)]
    pub struct PingMethod;

    impl crate::Method for PingMethod {
        type Req = PingRequest;

        type Res = PingResponse;

        type Error = Infallible;
    }

    impl Call for PingMethod {
        async fn call<T: futures::AsyncWrite + Unpin + Send + Sync, TransportError>(
            &mut self,
            replier: crate::Replier<'_, T, Self>,
            _value: Self::Req,
        ) -> Result<
            crate::transport::ReplyReceipt<Self::Res>,
            crate::ClientError<TransportError, Self::Error>,
        > {
            replier.reply(PingResponse::default()).await
        }
    }

    #[derive(Default, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct PingResponse(#[cbor(skip)] pub MethodWrapper<RootHandler>);

    impl std::fmt::Debug for PingResponse {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("PingResponse").field(&()).finish()
        }
    }

    #[derive(Default, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
    pub struct LogoutResponse(#[cbor(skip)] pub MethodWrapper<super::authenticate::LoginMethod>);

    #[derive(CborLen, maxlen::MaxLen, minicbor::Encode, minicbor::Decode)]
    pub enum NextHandler {
        #[n(0)]
        Actions(#[cbor(skip)] MethodWrapper<RootHandler>),
        #[n(1)]
        Authenticate(#[cbor(skip)] MethodWrapper<super::authenticate::LoginMethod>),
    }

    impl std::fmt::Debug for NextHandler {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Actions(_arg0) => f.debug_tuple("Actions").finish(),
                Self::Authenticate(_arg0) => f.debug_tuple("Authenticate").finish(),
            }
        }
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
    use actions::{LogoutMethod, LogoutRequest, PingMethod, PingRequest};
    use authenticate::{LoginMethod, Responses};
    let net = in_memory_transport::Network::new();

    let net1 = net.clone();
    let tp = MemoryTransport::new(net1, 1);
    let server_addr = tp.address();

    let ls = tokio::task::LocalSet::new();
    ls.run_until(async move {
        let mut js = JoinSet::new();
        // server
        js.spawn_local(async move {
            let mut login_wrapper: MethodWrapper<LoginMethod> = MethodWrapper::default();
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
                        Responses::Ok(method_wrapper) => break method_wrapper,
                        Responses::TryAgain(new_login_wrapper) => login_wrapper = new_login_wrapper,
                    }
                };

                login_wrapper = loop {
                    let stream = match conn.accept_stream().await {
                        Ok(stream) => stream,
                        Err(e) => {
                            warn!("{e}");
                            break 'outer;
                        }
                    };
                    match actions
                        .handle_state_transition_request(&conn, stream, &mut actions::RootHandler)
                        .await
                        .unwrap()
                    {
                        actions::NextHandler::Actions(method_wrapper) => actions = method_wrapper,
                        actions::NextHandler::Authenticate(method_wrapper) => break method_wrapper,
                    }
                }
            }
        });
        // client
        let _client_handle = js.spawn_local(async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let tp = MemoryTransport::new(net, 2);
            let conn = tp.connect(&server_addr).await.unwrap();

            let login_wrapper: MethodWrapper<LoginMethod> = Default::default();

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
                .query::<PingMethod, _>(PingRequest(), &conn)
                .await
                .unwrap()
                .0;

            let _login_wrapper = actions_wrapper
                .query::<LogoutMethod, _>(LogoutRequest(), &conn)
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
    use actions::{
        LogoutMethod, LogoutRequest, LongPingMethod, LongPingRequest, LoopbackHandler, PingMethod,
        PingRequest,
    };
    use authenticate::{LoginMethod, Responses};
    let net = in_memory_transport::Network::new();

    let net1 = net.clone();
    let tp = MemoryTransport::new(net1, 1);
    let server_addr = tp.address();

    let ls = tokio::task::LocalSet::new();

    ls.run_until(async move {
        let mut js = JoinSet::new();
        // server
        js.spawn_local(async move {
            let mut login_wrapper: MethodWrapper<LoginMethod> = MethodWrapper::default();
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
                        Responses::Ok(method_wrapper) => break method_wrapper,
                        Responses::TryAgain(new_login_wrapper) => login_wrapper = new_login_wrapper,
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
                            break root_handler_wrapper;
                        }
                    };
                };
            }
        });
        // client
        let _client_handle = js.spawn_local(async move {
            let tp = MemoryTransport::new(net, 2);
            let conn = tp.connect(&server_addr).await.unwrap();

            let login_wrapper: MethodWrapper<LoginMethod> = Default::default();

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

            let set = FuturesUnordered::new();

            let actions_wrapper_ref = &actions_wrapper;

            let now = Instant::now();

            set.push(
                actions_wrapper_ref.concurrent_query::<LongPingMethod, _, LoopbackHandler>(
                    LongPingRequest {
                        duration_millis: 100,
                    },
                    &conn,
                ),
            );

            set.push(
                actions_wrapper_ref.concurrent_query::<LongPingMethod, _, LoopbackHandler>(
                    LongPingRequest {
                        duration_millis: 100,
                    },
                    &conn,
                ),
            );

            let diff = now.elapsed();

            assert!(
                diff.as_millis() < 150,
                "should take less time than it would if requests were processed serially"
            );

            let _results = set.try_collect::<Vec<_>>().await.unwrap();

            let actions_wrapper = actions_wrapper
                .query::<PingMethod, _>(PingRequest(), &conn)
                .await
                .unwrap()
                .0;

            let _login_wrapper = actions_wrapper
                .query::<LogoutMethod, _>(LogoutRequest(), &conn)
                .await
                .unwrap()
                .0;
        });
        js.join_all().await;
    })
    .await;
}

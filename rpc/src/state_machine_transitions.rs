use std::marker::PhantomData;

use maxlen::MaxLen;

use crate::Caller;

pub struct MethodWrapper<Root: crate::RpcMessage, Handler: crate::RootHandler<Root>>(
    PhantomData<(Root, Handler)>,
);

impl<'b, C, Root: crate::RpcMessage, Handler: crate::RootHandler<Root>> minicbor::Decode<'b, C>
    for MethodWrapper<Root, Handler>
{
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        PhantomData::decode(d, ctx).map(|pd| MethodWrapper(pd))
    }
}

impl<C, Root: crate::RpcMessage, Handler: crate::RootHandler<Root>> minicbor::Encode<C>
    for MethodWrapper<Root, Handler>
{
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        PhantomData::encode(&self.0, e, ctx)
    }
}

impl<Root: crate::RpcMessage, Handler: crate::RootHandler<Root>> Default
    for MethodWrapper<Root, Handler>
{
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<C, Root: crate::RpcMessage, Handler: crate::RootHandler<Root>> minicbor::CborLen<C>
    for MethodWrapper<Root, Handler>
{
    fn cbor_len(&self, ctx: &mut C) -> usize {
        PhantomData::cbor_len(&self.0, ctx)
    }
}

impl<Root: crate::RpcMessage, Handler: crate::RootHandler<Root>> MaxLen
    for MethodWrapper<Root, Handler>
{
    fn biggest_instantiation() -> Self {
        Self::default()
    }
}

impl<Root: crate::RpcMessage, Handler: crate::RootHandler<Root>> MethodWrapper<Root, Handler> {
    pub fn handle_one_request<'a, C: crate::Client>(
        self,
        client: &'a C,
        stream: (C::SendStream, C::RecvStream),
        handler: &'a mut Handler,
    ) -> impl std::future::Future<
        Output = Result<Handler::Response, crate::ClientError<C::Error, Handler::Error>>,
    > + Send
    + 'a
    where
        Handler: 'a,
        Root: 'a,
    {
        client.handle_one_request::<Root, Handler>(stream, handler)
    }
    pub async fn query<M: crate::Method, C: Caller>(
        self,
        req: M::Req,
        caller: &C,
    ) -> Result<M::Res, crate::CallerError<C::Error>>
    where
        Root: crate::RpcMessage + From<M::Req> + Send,
    {
        caller.query::<M, Root>(req).await
    }
}

#[cfg(test)]
mod tests {
    use std::{default, panic};

    use tokio::task::JoinSet;
    use tracing::debug;

    use crate::{
        Client, Incoming, Transport,
        in_memory_transport::{self, MemoryTransport},
        state_machine_transitions::{
            MethodWrapper,
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
        use crate::{RootHandler, state_machine_transitions::MethodWrapper};

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
            Ok(#[cbor(skip)] MethodWrapper<actions::RootRequest, actions::RootHandler>),
            #[n(1)]
            TryAgain(#[cbor(skip)] MethodWrapper<LoginRequest, LoginMethod>),
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

        use crate::{Call, state_machine_transitions::MethodWrapper};

        #[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
        #[cbor(flat)]
        pub enum RootRequest {
            #[n(0)]
            Ping(),
            #[n(1)]
            Logout(),
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
                    RootRequest::Ping() => replier
                        .reply_with(&mut PingMethod, PingRequest())
                        .await?
                        .map(|r| NextHandler::Actions(r.0)),
                    RootRequest::Logout() => replier
                        .reply::<_, _, LogoutMethod>(LogoutResponse::default())
                        .await?
                        .map(|r| NextHandler::Authenticate(r.0)),
                })
            }
        }

        pub struct PingRequest();

        impl From<PingRequest> for RootRequest {
            fn from(_value: PingRequest) -> Self {
                Self::Ping()
            }
        }

        pub struct LogoutRequest();

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
        pub struct PingResponse(#[cbor(skip)] pub MethodWrapper<RootRequest, RootHandler>);

        impl std::fmt::Debug for PingResponse {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_tuple("PingResponse").field(&()).finish()
            }
        }

        #[derive(Default, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
        pub struct LogoutResponse(
            #[cbor(skip)]
            pub  MethodWrapper<super::authenticate::LoginRequest, super::authenticate::LoginMethod>,
        );

        pub enum NextHandler {
            Actions(MethodWrapper<RootRequest, RootHandler>),
            Authenticate(
                MethodWrapper<super::authenticate::LoginRequest, super::authenticate::LoginMethod>,
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

        let mut js = JoinSet::new();
        // server
        js.spawn(async move {
            let mut login_wrapper: MethodWrapper<authenticate::LoginRequest, LoginMethod> =
                MethodWrapper::default();
            let incoming = tp.accept().await.unwrap();
            let conn = incoming.accept().await.unwrap();
            'outer: loop {
                let mut actions = loop {
                    let Ok(stream) = conn.accept_stream().await else {
                        debug!("breaking out of server loop due to accept_stream error");
                        break 'outer;
                    };
                    match login_wrapper
                        .handle_one_request(&conn, stream, &mut LoginMethod)
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
                    let stream = conn.accept_stream().await.unwrap();
                    match actions
                        .handle_one_request(&conn, stream, &mut actions::RootHandler)
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
        let client_handle = js.spawn(async move {
            let tp = MemoryTransport::new(net);
            let conn = tp.connect(&server_addr).await.unwrap();

            let login_wrapper: MethodWrapper<
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
    }
}

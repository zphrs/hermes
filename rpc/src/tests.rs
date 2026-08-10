use std::convert::Infallible;

use tokio::task::JoinSet;

use crate::{
    RpcError, Transport,
    in_memory_transport::{self, MemoryTransport},
    method::Ancestor,
    traits::method::{can_transition::False, is_leaf},
    transport::{self, CallerExt as _, Client, ClientExt as _, Incoming},
};

#[derive(
    Debug,
    minicbor_derive::Encode,
    minicbor_derive::Decode,
    minicbor_derive::CborLen,
    maxlen::MaxLen,
)]
#[cbor(flat)]
pub enum Root {
    #[n(0)]
    Ping(#[n(0)] ping::Request),
    #[n(1)]
    Other(#[n(0)] other_ping::Request),
}

pub mod ping {

    use std::convert::Infallible;

    use crate::{method::is_leaf, traits::method::can_transition};
    #[derive(
        Debug,
        minicbor_derive::Encode,
        minicbor_derive::Decode,
        minicbor_derive::CborLen,
        maxlen::MaxLen,
    )]
    #[allow(dead_code)]
    pub struct Request;

    impl From<Request> for super::Root {
        fn from(value: Request) -> Self {
            Self::Ping(value)
        }
    }

    #[derive(
        Debug,
        minicbor_derive::Encode,
        minicbor_derive::Decode,
        minicbor_derive::CborLen,
        maxlen::MaxLen,
    )]
    pub struct Response;

    pub struct Method;

    impl crate::Method for Method {
        type Req = Request;
        type Res = Response;
        type CanTransition = can_transition::False;
        type IsLeaf = is_leaf::True;
    }

    impl crate::Handler<super::RootHandler> for Method {
        type Error = Infallible;

        async fn handle<Replier: crate::transport::ReplyHelper<Self, super::RootHandler>>(
            &mut self,
            replier: Replier,
            _value: <Self as crate::Method>::Req,
        ) -> Result<Replier::Receipt<Self>, crate::traits::HandleError<Replier::Error, Self::Error>>
        {
            replier.reply(Response).await
        }
    }
}

pub mod other_ping {

    use crate::{method::is_leaf, traits::method::can_transition};

    #[derive(
        Debug,
        minicbor_derive::Encode,
        minicbor_derive::Decode,
        minicbor_derive::CborLen,
        maxlen::MaxLen,
    )]
    pub struct Request;

    impl From<Request> for super::Root {
        fn from(value: Request) -> Self {
            Self::Other(value)
        }
    }

    #[derive(
        Debug,
        minicbor_derive::Encode,
        minicbor_derive::Decode,
        minicbor_derive::CborLen,
        maxlen::MaxLen,
    )]
    pub struct Response;

    pub struct Method;

    impl crate::Method for Method {
        type Req = Request;
        type Res = Response;
        type CanTransition = can_transition::False;
        type IsLeaf = is_leaf::True;
    }

    impl crate::Handler<super::RootHandler> for Method {
        type Error = std::convert::Infallible;

        async fn handle<Replier: crate::transport::ReplyHelper<Self, super::RootHandler>>(
            &mut self,
            replier: Replier,
            _value: <Self as crate::Method>::Req,
        ) -> Result<Replier::Receipt<Self>, crate::traits::HandleError<Replier::Error, Self::Error>>
        {
            replier.reply(Response).await
        }
    }
}

struct RootHandler;

impl crate::method::FromDescendant<ping::Method> for RootHandler {
    fn from_descendant_req(
        request: <ping::Method as crate::Method>::Req,
    ) -> <Self as crate::Method>::Req {
        Root::Ping(request)
    }

    fn from_descendant_res(
        _result: <ping::Method as crate::Method>::Res,
    ) -> <Self as crate::Method>::Res {
        unimplemented!()
    }

    fn try_into_descendant_req(
        _request: Self::Req,
    ) -> Result<<ping::Method as crate::Method>::Req, Self::Req> {
        unimplemented!()
    }
}

impl crate::Method for RootHandler {
    type Req = Root;

    type Res = ();
    type CanTransition = False;
    type IsLeaf = is_leaf::False;
}

impl Ancestor<other_ping::Method> for RootHandler {}

impl Ancestor<ping::Method> for RootHandler {}

#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("rpc: {0}")]
    Rpc(#[from] RpcError),
}

impl crate::Handler<RootHandler> for RootHandler {
    type Error = Infallible;

    async fn handle<Replier: transport::ReplyHelper<Self, RootHandler>>(
        &mut self,
        replier: Replier,
        value: <Self as crate::Method>::Req,
    ) -> Result<Replier::Receipt<Self>, crate::traits::HandleError<Replier::Error, Self::Error>>
    {
        match value {
            Root::Ping(request) => Ok(replier
                .reply_with(&mut ping::Method, request, |_v| ())
                .await?),
            Root::Other(request) => Ok(replier
                .reply_with(&mut other_ping::Method, request, |_v| ())
                .await?),
        }
    }
}
#[tokio::test]
async fn test() {
    let network = in_memory_transport::Network::new();

    let mut js = JoinSet::new();
    let net1 = network.clone();
    // server
    let tp = MemoryTransport::new(net1, 1u64);
    let server_addr = tp.address();
    js.spawn(async move {
        let incoming = tp.accept().await.unwrap();
        let conn = incoming.accept().await.unwrap();
        let mut stream = conn.accept_stream().await.unwrap();
        // let (write, mut read) = stream;
        // let replier = ImmediateReplier::from(write);
        // let mut root_handler = RootHandler;
        // let fut = conn
        //     .handle_one_request_with_handler::<ImmediateReplier<in_memory_transport::SendStream, _>, _, _>(
        //         (replier, &mut read), &mut root_handler,
        //     );
        // async move { fut.await.map(|v| v.into_inner()) }.await;
        let _ = conn
            .handle_one_request::<_, _, RootHandler>(&mut stream, &mut RootHandler)
            .await;
    });
    // client
    js.spawn(async move {
        let tp = MemoryTransport::new(network, 2u64);
        let conn = tp.connect(&server_addr).await.unwrap();
        let _res = conn
            .query::<ping::Method, RootHandler>(ping::Request)
            .await
            .unwrap();
    });
    js.join_all().await;
}

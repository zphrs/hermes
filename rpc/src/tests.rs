use std::convert::Infallible;

use maxlen::MaxLen;
use tokio::task::JoinSet;

use crate::{
    Call as _, RpcError, Transport,
    in_memory_transport::{self, MemoryTransport},
    transport::Caller,
    transport::{Client, Incoming},
};

use crate::transport::ReplyReceipt;

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

mod ping {
    use maxlen::MaxLen;
    use std::convert::Infallible;

    use crate::transport::ReplyReceipt;
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
        type Error = Infallible;
    }

    impl crate::Call for Method {
        async fn call<T: futures::AsyncWrite + Unpin + Send + Sync, TransportError>(
            &mut self,
            replier: crate::Replier<'_, T, Self>,
            _value: Self::Req,
        ) -> Result<ReplyReceipt<Self::Res>, crate::ClientError<TransportError, Self::Error>>
        {
            replier.reply(Response).await
        }
    }
}

mod other_ping {
    use maxlen::MaxLen;

    #[derive(
        Debug,
        minicbor_derive::Encode,
        minicbor_derive::Decode,
        minicbor_derive::CborLen,
        maxlen::MaxLen,
    )]
    pub struct Request;

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
        type Error = std::convert::Infallible;
    }

    impl crate::Call for Method {
        fn call<T: futures::AsyncWrite + Unpin + Send + Sync, TransportError>(
            &mut self,
            replier: crate::Replier<'_, T, Self>,
            _value: Self::Req,
        ) -> impl Future<
            Output = Result<
                crate::transport::ReplyReceipt<Self::Res>,
                crate::ClientError<TransportError, Self::Error>,
            >,
        > + Send {
            replier.reply(Response)
        }
    }
}

struct RootHandler;

impl crate::Method for RootHandler {
    type Req = Root;

    type Res = ();

    type Error = Infallible;
}
#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("rpc: {0}")]
    Rpc(#[from] RpcError),
}

impl crate::Call for RootHandler {
    async fn call<T: futures::AsyncWrite + Unpin + Send + Sync, TransportError>(
        &mut self,
        replier: crate::Replier<'_, T, Self>,
        value: Self::Req,
    ) -> Result<ReplyReceipt<Self::Res>, crate::ClientError<TransportError, Self::Error>> {
        match value {
            Root::Ping(request) => Ok(ping::Method
                .call(replier.change_method(&request), request)
                .await?
                .clear()),
            Root::Other(request) => Ok(other_ping::Method
                .call(replier.change_method(&request), request)
                .await?
                .clear()),
        }
    }
}
#[tokio::test]
async fn test() {
    let network = in_memory_transport::Network::new();

    let mut js = JoinSet::new();
    let net1 = network.clone();
    // server
    let tp = MemoryTransport::new(net1);
    let server_addr = tp.address();
    js.spawn(async move {
        let incoming = tp.accept().await.unwrap();
        let conn = incoming.accept().await.unwrap();
        let mut stream = conn.accept_stream().await.unwrap();
        let _ = conn.handle_one_request(&mut stream, &mut RootHandler).await;
    });
    // client
    js.spawn(async move {
        let tp = MemoryTransport::new(network);
        let conn = tp.connect(&server_addr).await.unwrap();
        let _res = conn
            .query::<ping::Method, Root>(ping::Request)
            .await
            .unwrap();
    });
    js.join_all().await;
}

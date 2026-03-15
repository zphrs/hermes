use derive_more::{From, TryInto};
use quic_rpc::{RpcClient, RpcServer, Service, message::RpcMsg};
use serde::{Deserialize, Serialize};

// Define your messages
#[derive(Debug, Serialize, Deserialize)]
struct Ping;

#[derive(Debug, Serialize, Deserialize)]
struct Pong;

// Define your RPC service and its request/response types
#[derive(Debug, Clone)]
struct PingService;

#[derive(Debug, Serialize, Deserialize, From, TryInto)]
enum PingRequest {
    Ping(Ping),
}

#[derive(Debug, Serialize, Deserialize, From, TryInto)]
enum PingResponse {
    Pong(Pong),
}

impl Service for PingService {
    type Req = PingRequest;
    type Res = PingResponse;
}

// Define interaction patterns for each request type
impl RpcMsg<PingService> for Ping {
    type Response = Pong;
}
#[tokio::test]
async fn test() -> anyhow::Result<()> {
    // create a transport channel, here a memory channel for testing
    let (server, client) = quic_rpc::transport::flume::channel(1);

    // client side
    // create the rpc client given the channel and the service type
    let client = RpcClient::<PingService, _>::new(client);

    // call the service
    let _res = client.rpc(Ping).await?;

    // server side
    // create the rpc server given the channel and the service type
    let server = RpcServer::<PingService, _>::new(server);

    let handler = Handler;
    loop {
        // accept connections
        let (msg, chan) = server.accept().await?.read_first().await?;
        // dispatch the message to the appropriate handler
        match msg {
            PingRequest::Ping(ping) => chan.rpc(ping, handler, Handler::ping).await?,
        }
    }

    // the handler. For a more complex example, this would contain any state
    // needed to handle the request.
    #[derive(Debug, Clone, Copy)]
    struct Handler;

    impl Handler {
        // the handle fn for a Ping request.

        // The return type is the response type for the service.
        // Note that this must take self by value, not by reference.
        async fn ping(self, _req: Ping) -> Pong {
            Pong
        }
    }
}

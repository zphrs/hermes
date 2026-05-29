pub mod as_sky;

use std::{convert::Infallible, net::IpAddr};

use maxlen::MaxLen;
use shared_schema::EarthNode;

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Request {
    #[n(0)]
    Sky(#[n(0)] as_sky::Request),
    #[n(1)]
    Earth(#[n(0)] EarthNode),
}

impl From<as_sky::Request> for Request {
    fn from(value: as_sky::Request) -> Self {
        Self::Sky(value)
    }
}

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Response {
    #[n(0)]
    Sky(#[n(0)] as_sky::Response),
}

pub struct Method {
    peer_ip: IpAddr,
}

impl Method {
    pub fn new(peer_ip: IpAddr) -> Self {
        Self { peer_ip }
    }
}

impl rpc::Method for Method {
    type Req = Request;

    type Res = Response;

    type Error = Infallible;
}

impl rpc::Call for Method {
    async fn call<T: futures_io::AsyncWrite + Unpin + Send + Sync, TransportError>(
        &mut self,
        replier: rpc::Replier<'_, T, Self>,
        value: Self::Req,
    ) -> Result<rpc::ReplyReceipt<Self::Res>, rpc::ClientError<TransportError, Self::Error>> {
        Ok(match value {
            Request::Sky(mut request) => {
                request.set_sky_node(self.peer_ip);
                as_sky::Method
                    .call(replier.change_method(&request), request)
                    .await?
                    .map(Response::Sky)
            }
            Request::Earth(earth_node) => todo!(),
        })
    }
}

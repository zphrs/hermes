pub mod as_earth;
pub mod as_sky;

use std::{convert::Infallible, net::IpAddr};

use maxlen::MaxLen;
use rpc::MethodWrapper;
use shared_schema::{EarthNode, SkyNode};

pub type Entrypoint = MethodWrapper<Method>;

#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub enum Request {
    #[n(0)]
    Sky(#[n(0)] as_sky::Request),
    #[n(1)]
    Earth(#[n(0)] as_earth::Request),
}

impl From<as_sky::Request> for Request {
    fn from(value: as_sky::Request) -> Self {
        Self::Sky(value)
    }
}

impl From<as_earth::Request> for Request {
    fn from(value: as_earth::Request) -> Self {
        Self::Earth(value)
    }
}

pub enum Response {
    Sky(as_sky::Response, SkyNode),
    Earth(as_earth::Response, EarthNode),
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
                    .map(|v| Response::Sky(v, SkyNode::from(self.peer_ip)))
            }
            Request::Earth(earth_node) => {
                let mut handler = as_earth::Method::new();
                replier
                    .change_method(&earth_node)
                    .reply_with(&mut handler, earth_node.clone())
                    .await?
                    .map(|v| Response::Earth(v, earth_node))
            }
        })
    }
}

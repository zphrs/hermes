pub mod as_earth;
pub mod as_sky;

use std::{convert::Infallible, net::IpAddr};

use maxlen::MaxLen;
use rpc::{
    state::priority::ServerWins,
    traits::method::{can_transition, not_applicable::NotApplicable},
};
use shared_schema::{EarthNode, SkyNode};

pub struct EntrypointState;

pub type Entrypoint = ServerWins<EntrypointState>;

impl rpc::traits::State for EntrypointState {
    type ClientHandles = NotApplicable;

    type ServerHandles = Method;
}

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

    type CanTransition = can_transition::True;
}

impl rpc::Handler for Method {
    type Error = Infallible;

    async fn handle<Replier: rpc::transport::ReplyHelper<Self>>(
        &mut self,
        replier: Replier,
        value: <Self as rpc::Method>::Req,
    ) -> Result<
        <Replier as rpc::transport::ReplyHelper<Self>>::Receipt<Self>,
        rpc::traits::HandleError<
            <Replier as rpc::transport::ReplyHelper<Self>>::Error,
            <Self as rpc::Handler<Self>>::Error,
        >,
    > {
        Ok(match value {
            Request::Sky(mut request) => {
                request.set_sky_node(self.peer_ip);
                replier
                    .reply_with(&mut as_sky::Method, request, |v| {
                        Response::Sky(v, SkyNode::from(self.peer_ip))
                    })
                    .await?
            }
            Request::Earth(earth_node) => {
                let mut handler = as_earth::Method::new();
                replier
                    .reply_with(&mut handler, earth_node.clone(), |v| {
                        Response::Earth(v, earth_node)
                    })
                    .await?
            }
        })
    }
}

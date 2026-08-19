pub mod as_earth;
pub mod as_sky;

use std::{convert::Infallible, net::IpAddr};

use maxlen::MaxLen;
use rpc::{
    method::{Ancestor, is_leaf},
    traits::method::{can_transition, not_applicable::NotApplicable},
};
use shared_schema::{EarthNode, SkyNode};

pub struct EntrypointState;

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

    type IsLeaf = is_leaf::False;
}

impl<RM: Ancestor<Self> + Ancestor<as_sky::Method> + Ancestor<as_earth::Method>> rpc::Handler<RM>
    for Method
{
    type Error = Infallible;

    async fn handle<Replier: rpc::ReplyHelper<RM, Self>>(
        &mut self,
        replier: Replier,
        value: rpc::ReqOf<Self>,
    ) -> rpc::traits::HandlerResult<RM, Self, Replier, Self::Error> {
        Ok(match value {
            Request::Sky(mut request) => {
                request.set_sky_node(self.peer_ip);
                replier
                    .reply_with::<as_sky::Method, _>(&mut as_sky::Method, request, |v| {
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

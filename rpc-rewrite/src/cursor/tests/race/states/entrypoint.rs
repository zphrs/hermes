use std::time::Duration;

use crate::{cursor::state, marker::Transition, method::handler::root_method::RootMethod};

pub struct State;

impl state::State for State {
    type ClientBranchType = Transition;
    type ClientHandles = RootMethod<ServerRequestWins, Transition>;

    type ServerBranchType = Transition;
    type ServerHandles = RootMethod<ClientRequestWins, Transition>;
}

pub struct ServerRequestWins;

impl crate::Method for ServerRequestWins {
    type Req<'buf> = Duration;
    type Res<'buf> = state::Wrapper<super::winner::server::State>;

    type Type = crate::method::LeafTransition;
}

pub struct ClientRequestWins;

impl crate::Method for ClientRequestWins {
    type Req<'buf> = Duration;
    type Res<'buf> = state::Wrapper<super::winner::client::State>;

    type Type = crate::method::LeafTransition;
}

use std::time::Duration;

use crate::{
    cursor::state,
    markers::{False, True},
    method::handler::root_method::RootMethod,
};

pub struct State;

impl state::State for State {
    type ClientHandles = RootMethod<ServerRequestWins>;

    type ServerHandles = RootMethod<ClientRequestWins>;
}

pub struct ServerRequestWins;

impl crate::Method for ServerRequestWins {
    type Req<'buf> = Duration;

    type Res<'buf> = state::Wrapper<super::winner::server::State>;

    type Transitions = True;

    type HasDescendants = False;
}

pub struct ClientRequestWins;

impl crate::Method for ClientRequestWins {
    type Req<'buf> = Duration;

    type Res<'buf> = state::Wrapper<super::winner::client::State>;

    type Transitions = True;

    type HasDescendants = False;
}

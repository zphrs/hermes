use std::time::Duration;

use crate::{
    cursor::tests::race::states,
    traits::{
        self,
        handler::root_method::RootMethod,
        markers::{False, True},
        state,
    },
};

pub struct State;

impl state::Entrypoint for states::entrypoint::State {}

impl traits::State for State {
    type ClientHandles = RootMethod<ServerRequestWins>;

    type ServerHandles = RootMethod<ClientRequestWins>;
}

pub struct ServerRequestWins;

impl traits::Method for ServerRequestWins {
    type Req<'buf> = Duration;

    type Res<'buf> = state::Wrapper<super::winner::server::State>;

    type Transitions = True;

    type HasDescendants = False;
}

pub struct ClientRequestWins;

impl traits::Method for ClientRequestWins {
    type Req<'buf> = Duration;

    type Res<'buf> = state::Wrapper<super::winner::client::State>;

    type Transitions = True;

    type HasDescendants = False;
}

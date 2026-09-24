use rpc_rewrite::{
    cursor::state,
    marker::{Loopback, NotApplicable, Transition},
    method::handler::root_method::RootMethod,
};

pub mod join_room;

/// Tiebreak: no priority mechanism in rpc-rewrite (old: `server_wins`).
pub struct Entrypoint;

impl state::State for Entrypoint {
    type ClientBranchType = Loopback;
    type ClientHandles = NotApplicable;

    type ServerBranchType = Transition;
    type ServerHandles = RootMethod<join_room::JoinRoom, Transition>;
}

impl state::Entrypoint for Entrypoint {}

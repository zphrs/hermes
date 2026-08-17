use rpc::{define_prioritized, method::not_applicable::NotApplicable, state::priority};

pub struct Entrypoint;

impl rpc::State for Entrypoint {
    type ClientHandles = NotApplicable;

    type ServerHandles = entrypoint::join_room::JoinRoom;
}

define_prioritized!(Entrypoint, priority::server_wins);

pub mod entrypoint;

pub mod in_room;

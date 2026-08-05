pub mod login;

use rpc::{
    define_prioritized, method::not_applicable::NotApplicable, state::priority::server_wins,
};
pub struct Entrypoint;

impl rpc::State for Entrypoint {
    type ClientHandles = NotApplicable;

    type ServerHandles = login::Method;
}

define_prioritized!(Entrypoint, server_wins);

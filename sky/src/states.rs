use rpc::method::not_applicable;

pub struct Entrypoint;

impl rpc::State for Entrypoint {
    type ClientHandles = not_applicable::NotApplicable;

    type ServerHandles = login::Method;
}

pub mod login;

pub mod sky;

pub mod earth;

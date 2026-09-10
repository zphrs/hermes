use crate::cursor::state;

pub mod winner {
    pub mod client;

    pub mod server;
}

pub mod entrypoint;
impl state::Entrypoint for entrypoint::State {}

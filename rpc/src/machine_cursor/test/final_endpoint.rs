use crate::{MachineCursor, in_memory_transport};

pub use super::prelude::*;

pub enum FinalEndpoint<Role: crate::state::Role> {
    Client(MachineCursor<ClientEndpoint, in_memory_transport::Connection<u8>, Role>),
    Server(MachineCursor<ServerEndpoint, in_memory_transport::Connection<u8>, Role>),
}

impl<Role: crate::state::Role> std::fmt::Debug for FinalEndpoint<Role> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Client(_) => f.debug_tuple("Client").finish(),
            Self::Server(_) => f.debug_tuple("Server").finish(),
        }
    }
}

impl<Role1: crate::state::Role, Role2: crate::state::Role> PartialEq<FinalEndpoint<Role1>>
    for FinalEndpoint<Role2>
{
    fn eq(&self, other: &FinalEndpoint<Role1>) -> bool {
        match (self, other) {
            (Self::Client(_), FinalEndpoint::Client(_)) => true,
            (Self::Server(_), FinalEndpoint::Server(_)) => true,
            _ => false,
        }
    }
}

impl<Role: crate::state::Role>
    From<MachineCursor<ServerEndpoint, in_memory_transport::Connection<u8>, Role>>
    for FinalEndpoint<Role>
{
    fn from(v: MachineCursor<ServerEndpoint, in_memory_transport::Connection<u8>, Role>) -> Self {
        Self::Server(v)
    }
}

impl<Role: crate::state::Role>
    From<MachineCursor<ClientEndpoint, in_memory_transport::Connection<u8>, Role>>
    for FinalEndpoint<Role>
{
    fn from(v: MachineCursor<ClientEndpoint, in_memory_transport::Connection<u8>, Role>) -> Self {
        Self::Client(v)
    }
}

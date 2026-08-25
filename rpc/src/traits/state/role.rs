//! Marker types for each side

#[derive(Clone, Copy)]
pub struct Client;
#[derive(Clone, Copy)]
pub struct Server;

pub(crate) trait Role: Copy {
    fn to_enum() -> WhichRole;
}

pub enum WhichRole {
    Client,
    Server,
}

impl Role for Client {
    #[inline(always)]
    fn to_enum() -> WhichRole {
        WhichRole::Client
    }
}

impl Role for Server {
    #[inline(always)]
    fn to_enum() -> WhichRole {
        WhichRole::Server
    }
}

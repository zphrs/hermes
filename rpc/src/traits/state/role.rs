//! Marker types for each side

#[derive(Clone, Copy)]
pub struct Client;
#[derive(Clone, Copy)]
pub struct Server;

/// scoped to pub(crate) to ensure the only two implementations of
/// Role are Client and Server
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

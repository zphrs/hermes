use crate::traits::markers::{Client, Server};

pub enum Role {
    Client,
    Server,
}
/// Either [`Client`] or [`Server`]. pub(crate) to ensure no other type
/// implements it.
pub(crate) trait Sealed {
    fn as_enum() -> Role;
}

impl Sealed for Client {
    fn as_enum() -> Role {
        Role::Client
    }
}
impl Sealed for Server {
    fn as_enum() -> Role {
        Role::Server
    }
}

pub mod client;
pub mod server;

/// tiebreaks based on request integer
pub struct Entrypoint;

impl crate::State for Entrypoint {
    type ClientMethod = client::Method;

    type ServerMethod = server::Method;
}

impl crate::state::Prioritized for Entrypoint {
    type Priority = super::Priority;

    fn client_priority(request: &<Self::ClientMethod as crate::Method>::Req) -> Self::Priority {
        request.priority
    }

    fn server_priority(request: &<Self::ServerMethod as crate::Method>::Req) -> Self::Priority {
        request.priority
    }
}

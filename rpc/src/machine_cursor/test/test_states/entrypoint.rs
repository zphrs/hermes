pub mod client;
pub mod server;

/// tiebreaks based on request integer
pub struct Entrypoint;

impl crate::State for Entrypoint {
    type ClientHandles = client::Method;

    type ServerHandles = server::Method;
}

impl crate::state::Prioritized for Entrypoint {
    type Priority = super::Priority;

    fn client_priority(request: &<Self::ClientHandles as crate::Method>::Req) -> Self::Priority {
        request.priority
    }

    fn server_priority(request: &<Self::ServerHandles as crate::Method>::Req) -> Self::Priority {
        request.priority
    }
}

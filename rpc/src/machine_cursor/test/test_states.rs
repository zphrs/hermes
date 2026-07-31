pub mod client_endpoint;
pub mod entrypoint;
pub mod server_endpoint;

use std::time::Duration;

pub use entrypoint::Entrypoint;
use maxlen::MaxLen;

pub type Priority = bool;

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct Request {
    #[n(0)]
    pub priority: Priority,
    #[n(1)]
    pub sleep: Option<Duration>,
}

impl Request {
    pub const fn new(priority: Priority) -> Self {
        Self {
            priority,
            sleep: None,
        }
    }

    pub const fn with_sleep(mut self, sleep: Duration) -> Self {
        self.sleep = Some(sleep);
        self
    }
}

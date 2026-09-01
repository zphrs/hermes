mod boolean;
pub mod not_applicable;
mod role;

pub use boolean::{False, True};
pub use not_applicable::NotApplicable;
pub use role::{Client, Server};

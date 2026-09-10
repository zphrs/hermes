//! rpc with various improvements. Will be renamed into rpc once it has feature parity.
//! improvements include:
//! - reduced need to copy bytes around during de/serialization
//! - code cleanup
//! - more ergonomic traits
//! - fewer custom async machines
//! - no need for types to have a definite maximum size

mod io;
pub mod markers;
pub mod method;
#[cfg(test)]
mod quinn_transport;

pub mod cursor;

pub use method::Method;

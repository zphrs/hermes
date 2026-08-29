//! rpc with various improvements. Will be renamed into rpc once it has feature parity.
//! improvements include:
//! - reduced need to copy memory/bytes from the wire during de/serialization
//! - code cleanup
//! - more ergonomic traits
//! - fewer custom async machines
//! - no need for types to have a definite maximum size
//! -

#[cfg(test)]
mod quinn_transport;
pub mod traits;

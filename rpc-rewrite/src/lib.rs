//! rpc with various improvements. Will be renamed into rpc once it has feature parity.
//! improvements include:
//! - reduced need to copy bytes around during de/serialization
//! - code cleanup
//! - more ergonomic traits
//! - fewer custom async machines
//! - no need for types to have a definite maximum size

// lets `rpc_rewrite::...` paths resolve inside this crate's own tests (the room
// example is shared with them via `#[path]`)
#[cfg(test)]
extern crate self as rpc_rewrite;

mod io;
pub mod marker;
pub mod method;
#[cfg(feature = "quinn-transport")]
pub mod quinn_transport;

pub mod cursor;

pub use method::Method;

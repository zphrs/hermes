#![feature(ip_as_octets)]
#![feature(iter_array_chunks)]

mod capnp_types;
mod listener;
pub mod node;
pub mod server;

capnp::generated_code!(mod sky_capnp);

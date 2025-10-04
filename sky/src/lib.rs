pub mod server;
mod listener;

use capnp_futures;

capnp::generated_code!(mod sky_capnp);

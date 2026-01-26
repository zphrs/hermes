//! based on the [api of simgrid](https://simgrid.org/doc/latest/app_s4u.html)
mod udp;
pub mod ip;

pub use udp::Packet;

pub struct Host {}

pub struct Network {
    hosts: Vec<Host>,
}

pub struct Sim {
    root_network: Network,
}

#[cfg(test)]
mod tests {

    #[test]
    fn it_works() {}
}

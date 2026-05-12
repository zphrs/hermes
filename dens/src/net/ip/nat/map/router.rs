use std::{collections::HashMap, net::SocketAddr, time::Duration};

use rand::Rng as _;
use tokio::time::Instant;

use super::Map;
use crate::Sim;

use super::Mappable;

pub struct Router<RouterDependence: Mappable + Into<SocketAddr> + From<(SocketAddr, SocketAddr)>> {
    outgoing: HashMap<RouterDependence, u16>,
    incoming: HashMap<u16, (RouterDependence, Instant)>,
}

impl<RouterDependence: Mappable + Into<SocketAddr> + From<(SocketAddr, SocketAddr)>> Default
    for Router<RouterDependence>
{
    fn default() -> Self {
        Self {
            outgoing: Default::default(),
            incoming: Default::default(),
        }
    }
}

impl<
    RouterDependence: Mappable + Into<SocketAddr> + From<(std::net::SocketAddr, std::net::SocketAddr)>,
> Map for Router<RouterDependence>
{
    fn external_port_to_internal_addr(
        &mut self,
        port: u16,
        _dst: SocketAddr,
    ) -> Option<SocketAddr> {
        let res = self.incoming.get_mut(&port)?;
        let now = Instant::now();
        let since_last_used = now.duration_since(res.1);
        if since_last_used > Duration::from_secs(30) {
            self.outgoing.remove(&res.0);
            self.incoming.remove(&port);
            return None;
        };

        res.1 = now;

        return Some(res.0.into());
    }

    fn internal_addr_to_external_port(&mut self, src: SocketAddr, dst: SocketAddr) -> u16 {
        let now = Instant::now();
        let addr: RouterDependence = (src, dst).into();
        let out = self.outgoing.entry(addr).or_insert_with(|| {
            let mut port: u16 = Sim::with_rng(|rng| rng.random());

            let wrapped_around_addr = port - 1;

            // if taken, loop around u16 with wrapping add
            while let Some(existing) = self.incoming.get_mut(&port) {
                // if last used more than 30 secs ago, we can remove
                // from map and replace with out
                let since_last_used = now.duration_since(existing.1);
                if since_last_used > Duration::from_secs(30) {
                    self.incoming.remove(&port);
                    return port;
                }
                // make sure we aren't stuck in an infinite
                // loop in the case that all ports are taken
                if port == wrapped_around_addr {
                    panic!("all NAT addresses taken");
                };
                port = port.wrapping_add(1);
            }

            port
        });

        self.incoming.insert(*out, (addr, now));
        *out
    }
}

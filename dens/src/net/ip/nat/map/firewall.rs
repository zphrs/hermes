use super::Mappable;
use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};

pub struct Firewall<Dst: From<SocketAddr> + Mappable>(HashMap<(SocketAddr, Dst), Instant>);

impl<Dst: From<SocketAddr> + Mappable> Default for Firewall<Dst> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<Dst: From<SocketAddr> + Mappable> Firewall<Dst> {
    pub fn check(&mut self, src: SocketAddr, dst: impl Into<Dst>) -> bool {
        let dst: Dst = dst.into();
        let Some(last_used) = self.0.get(&(src, dst)) else {
            return false;
        };
        let since_last_used = Instant::now().duration_since(*last_used);
        if since_last_used > Duration::from_secs(30) {
            self.0.remove(&(src, dst));
            return false;
        };
        true
    }

    pub fn insert(&mut self, src: SocketAddr, dst: impl Into<Dst>) {
        self.0.insert((src, dst.into()), Instant::now());
    }
}

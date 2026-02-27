use std::{
    collections::HashMap,
    io::{self, ErrorKind},
    net::IpAddr,
};

pub struct Dns {
    lookup: HashMap<String, Vec<IpAddr>>,
}

// By default, initializes with only the loopback address for ipv4 & ipv6
impl Default for Dns {
    fn default() -> Self {
        let mut out = Self {
            lookup: Default::default(),
        };

        out.extend(
            "localhost",
            ["127.0.0.1", "::1"]
                .iter()
                .filter_map(|addr| addr.parse().ok()),
        );
        out
    }
}

impl Dns {
    pub fn new() -> Self {
        Default::default()
    }
    pub fn lookup(&self, domain: &str) -> io::Result<Vec<IpAddr>> {
        // ... and make the system look up the host.
        Ok(self.lookup.get(domain).ok_or(ErrorKind::NotFound)?.clone())
    }

    pub fn insert(&mut self, domain: &str, addr: IpAddr) {
        self.lookup.entry(domain.into()).or_default().push(addr);
    }
    pub fn extend(&mut self, domain: &str, addr: impl IntoIterator<Item = IpAddr>) {
        self.lookup.entry(domain.into()).or_default().extend(addr);
    }
}

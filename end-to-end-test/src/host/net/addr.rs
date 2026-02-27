use crate::host::net::addr::sealed::OneOrMore;
use crate::host::net::dns::Dns;
use crate::host::net::macros::cfg_net;

use crate::sim::SIM;
use std::future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

/// Converts or resolves without blocking to one or more `SocketAddr` values.
///
/// # DNS
///
/// Implementations of `ToSocketAddrs` for string types require a DNS lookup.
///
/// # Calling
///
/// Currently, this trait is only used as an argument to Tokio functions that
/// need to reference a target socket address. To perform a `SocketAddr`
/// conversion directly, use [`lookup_host()`](super::lookup_host()).
///
/// This trait is sealed and is intended to be opaque. The details of the trait
/// will change. Stabilization is pending enhancements to the Rust language.
pub trait ToSocketAddrs: sealed::ToSocketAddrsPriv {}

type ReadyFuture<T> = future::Ready<io::Result<T>>;

pub(crate) fn to_socket_addrs<T>(arg: T) -> io::Result<T::Iter>
where
    T: ToSocketAddrs,
{
    SIM.with(|sim| arg.to_socket_addrs(&mut sim.dns_mut()))
}

// ===== impl &impl ToSocketAddrs =====

impl<T: ToSocketAddrs + ?Sized> ToSocketAddrs for &T {}

impl<T> sealed::ToSocketAddrsPriv for &T
where
    T: sealed::ToSocketAddrsPriv + ?Sized,
{
    type Iter = T::Iter;

    fn to_socket_addrs(&self, dns: &mut Dns) -> io::Result<Self::Iter> {
        (**self).to_socket_addrs(dns)
    }
}

// ===== impl SocketAddr =====

impl ToSocketAddrs for SocketAddr {}

impl sealed::ToSocketAddrsPriv for SocketAddr {
    type Iter = std::option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self, _dns: &mut Dns) -> io::Result<Self::Iter> {
        let iter = Some(*self).into_iter();
        Ok(iter)
    }
}

// ===== impl SocketAddrV4 =====

impl ToSocketAddrs for SocketAddrV4 {}

impl sealed::ToSocketAddrsPriv for SocketAddrV4 {
    type Iter = std::option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self, dns: &mut Dns) -> io::Result<Self::Iter> {
        SocketAddr::V4(*self).to_socket_addrs(dns)
    }
}

// ===== impl SocketAddrV6 =====

impl ToSocketAddrs for SocketAddrV6 {}

impl sealed::ToSocketAddrsPriv for SocketAddrV6 {
    type Iter = std::option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self, dns: &mut Dns) -> io::Result<Self::Iter> {
        SocketAddr::V6(*self).to_socket_addrs(dns)
    }
}

// ===== impl (IpAddr, u16) =====

impl ToSocketAddrs for (IpAddr, u16) {}

impl sealed::ToSocketAddrsPriv for (IpAddr, u16) {
    type Iter = std::option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self, _: &mut Dns) -> io::Result<Self::Iter> {
        let iter = Some(SocketAddr::from(*self)).into_iter();
        Ok(iter)
    }
}

// ===== impl (Ipv4Addr, u16) =====

impl ToSocketAddrs for (Ipv4Addr, u16) {}

impl sealed::ToSocketAddrsPriv for (Ipv4Addr, u16) {
    type Iter = std::option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self, dns: &mut Dns) -> io::Result<Self::Iter> {
        let (ip, port) = *self;
        SocketAddrV4::new(ip, port).to_socket_addrs(dns)
    }
}

// ===== impl (Ipv6Addr, u16) =====

impl ToSocketAddrs for (Ipv6Addr, u16) {}

impl sealed::ToSocketAddrsPriv for (Ipv6Addr, u16) {
    type Iter = std::option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self, dns: &mut Dns) -> io::Result<Self::Iter> {
        let (ip, port) = *self;
        SocketAddrV6::new(ip, port, 0, 0).to_socket_addrs(dns)
    }
}

// ===== impl &[SocketAddr] =====

impl ToSocketAddrs for &[SocketAddr] {}

impl sealed::ToSocketAddrsPriv for &[SocketAddr] {
    type Iter = std::vec::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self, _dns: &mut Dns) -> io::Result<Self::Iter> {
        #[inline]
        fn slice_to_vec(addrs: &[SocketAddr]) -> Vec<SocketAddr> {
            addrs.to_vec()
        }

        // This uses a helper method because clippy doesn't like the `to_vec()`
        // call here (it will allocate, whereas `self.iter().copied()` would
        // not), but it's actually necessary in order to ensure that the
        // returned iterator is valid for the `'static` lifetime, which the
        // borrowed `slice::Iter` iterator would not be.
        //
        // Note that we can't actually add an `allow` attribute for
        // `clippy::unnecessary_to_owned` here, as Tokio's CI runs clippy lints
        // on Rust 1.52 to avoid breaking LTS releases of Tokio. Users of newer
        // Rust versions who see this lint should just ignore it.
        let iter = slice_to_vec(self).into_iter();
        Ok(iter)
    }
}

cfg_net! {
    // ===== impl str =====

    impl ToSocketAddrs for str {}

    impl sealed::ToSocketAddrsPriv for str {
        type Iter = OneOrMore;

        fn to_socket_addrs(&self, dns: &mut Dns) -> io::Result<Self::Iter> {
            // First check if the input parses as a socket address
            let res: Result<SocketAddr, _> = self.parse();

            if let Ok(addr) = res {
                return Ok(addr.into());
            }

            // Split the string by ':' and convert the second part to u16...
            let Some((host, port_str)) = self.rsplit_once(':') else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid socket address",
                ));
            };
            let Ok(port) = port_str.parse::<u16>() else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid port value",
                ));
            };

            // Run DNS lookup on the blocking pool
            Ok(OneOrMore::More(
                dns
                .lookup(host)?
                .into_iter()
                .map(|ip|SocketAddr::new(ip, port))
                .collect::<Vec<_>>()
                .into_iter()
            ))
        }
    }

    // ===== impl (&str, u16) =====

    impl ToSocketAddrs for (&str, u16) {}

    impl sealed::ToSocketAddrsPriv for (&str, u16) {
        type Iter = sealed::OneOrMore;

        fn to_socket_addrs(&self, dns: &mut Dns) -> io::Result<Self::Iter> {

            let (host, port) = *self;

            // try to parse the host as a regular IP address first
            if let Ok(addr) = host.parse::<Ipv4Addr>() {
                let addr = SocketAddrV4::new(addr, port);
                let addr = SocketAddr::V4(addr);

                return Ok(addr.into());
            }

            if let Ok(addr) = host.parse::<Ipv6Addr>() {
                let addr = SocketAddrV6::new(addr, port, 0, 0);
                let addr = SocketAddr::V6(addr);

                return Ok(addr.into());
            }


            Ok(OneOrMore::More(
                dns
                .lookup(host)?
                .into_iter()
                .map(|ip|SocketAddr::new(ip, port))
                .collect::<Vec<_>>()
                .into_iter()
            ))
        }
    }

    // ===== impl (String, u16) =====

    impl ToSocketAddrs for (String, u16) {}

    impl sealed::ToSocketAddrsPriv for (String, u16) {
        type Iter = sealed::OneOrMore;

        fn to_socket_addrs(&self, dns: &mut Dns) -> io::Result<Self::Iter> {
            (self.0.as_str(), self.1).to_socket_addrs(dns)
        }
    }

    // ===== impl String =====

    impl ToSocketAddrs for String {}

    impl sealed::ToSocketAddrsPriv for String {
        type Iter = <str as sealed::ToSocketAddrsPriv>::Iter;

        fn to_socket_addrs(&self, dns: &mut Dns) -> io::Result<Self::Iter> {
            self[..].to_socket_addrs(dns)
        }
    }
}

pub(crate) mod sealed {
    //! The contents of this trait are intended to remain private and __not__
    //! part of the `ToSocketAddrs` public API. The details will change over
    //! time.
    use std::net::SocketAddr;
    use std::{io, option, vec};

    use crate::host::net::dns::Dns;

    #[doc(hidden)]
    pub trait ToSocketAddrsPriv {
        type Iter: Iterator<Item = SocketAddr> + 'static;
        fn to_socket_addrs(&self, internal: &mut Dns) -> io::Result<Self::Iter>;
    }

    #[doc(hidden)]
    #[derive(Debug)]
    pub enum OneOrMore {
        One(option::IntoIter<SocketAddr>),
        More(vec::IntoIter<SocketAddr>),
    }

    impl Iterator for OneOrMore {
        type Item = SocketAddr;

        fn next(&mut self) -> Option<Self::Item> {
            match self {
                OneOrMore::One(i) => i.next(),
                OneOrMore::More(i) => i.next(),
            }
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            match self {
                OneOrMore::One(i) => i.size_hint(),
                OneOrMore::More(i) => i.size_hint(),
            }
        }
    }

    impl From<SocketAddr> for OneOrMore {
        fn from(value: SocketAddr) -> Self {
            OneOrMore::One(Some(value).into_iter())
        }
    }
}

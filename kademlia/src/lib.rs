#![feature(ip_as_octets)]
#![feature(slice_as_array)]
#![feature(debug_closure_helpers)]
#![feature(iter_array_chunks)]
#![feature(cold_path)]
#![feature(trait_alias)]

mod id;
pub mod prelude;
mod routing_table;
mod rpc;
mod traits;
pub use rpc::RpcManager;

pub use traits::{HasId, RequestHandler};

pub use id::{Distance, DistancePair, Id};

pub use routing_table::{Bucket, RoutingTable};

pub const BUCKET_SIZE: usize = 20;

#[cfg(test)]
mod node {
    use std::{fmt::from_fn, net::SocketAddr};

    use sha2::{Digest as _, Sha256};

    use crate::{HasId, id::Id};
    use std::fmt::Debug;

    #[derive(Clone)]
    pub struct Node {
        addr: SocketAddr,
        id: Id<256>,
    }

    impl HasId<256> for Node {
        fn id(&self) -> &Id<256> {
            &self.id
        }
    }

    impl Debug for Node {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Node")
                .field("addr", &self.addr)
                .field("id", &from_fn(|f| write!(f, "{}", self.id)))
                .finish()
        }
    }
    impl PartialEq for Node {
        fn eq(&self, other: &Self) -> bool {
            self.addr == other.addr
        }
    }

    impl Eq for Node {}

    impl Node {
        pub fn new(addr: SocketAddr) -> Self {
            let mut hasher = Sha256::new();
            hasher.update(addr.ip().as_octets());
            hasher.update(addr.port().to_be_bytes());
            let hash = hasher.finalize();
            Self {
                addr,
                id: hash.as_slice().as_array::<32>().unwrap().to_owned().into(),
            }
        }
    }
}

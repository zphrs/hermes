mod id;
pub mod prelude;
mod routing_table;
mod rpc;
mod traits;

mod helpers;
#[cfg(test)]
mod rpc_test;

pub use rpc::RpcManager;

pub use traits::{HasId, RequestHandler};

pub use id::{Distance, DistancePair, Id};

pub use routing_table::RoutingTable;

pub const BUCKET_SIZE: usize = 20;

#[cfg(test)]
mod node {
    use std::net::SocketAddr;

    use sha2::{Digest as _, Sha256};

    use crate::{HasId, id::Id};
    use std::fmt::Debug;

    #[derive(Clone)]
    pub struct Node {
        addr: SocketAddr,
        id: Id<32>,
    }

    impl HasId<32> for Node {
        fn id(&self) -> &Id<32> {
            &self.id
        }
    }

    impl Debug for Node {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Node")
                .field("addr", &self.addr)
                .field("id", &self.id.to_string())
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
            hasher.update(addr.ip().to_string());
            hasher.update(addr.port().to_be_bytes());
            let hash = hasher.finalize();
            Self {
                addr,
                id: hash.as_slice().into(),
            }
        }
    }
}

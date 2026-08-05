use futures::future::join;

use crate::{Transport as _, in_memory_transport, transport::Incoming as _};

#[must_use]
pub struct ConnPair {
    pub server_conn: in_memory_transport::Connection<u8>,
    pub client_conn: in_memory_transport::Connection<u8>,
}

pub async fn setup_conn(
    server_address: u8,
    client_address: u8,
    net: &in_memory_transport::Network<u8>,
) -> ConnPair {
    let server = async {
        let tp = net.new_transport(server_address);
        let incoming = tp.accept().await.expect("infallible");
        let conn = incoming.accept().await.expect("successful incoming");
        conn
    };

    let client = async {
        let tp = net.new_transport(client_address);
        let conn = tp.connect(&server_address).await.expect("infallible");
        conn
    };

    let (server_conn, client_conn) = join(server, client).await;

    ConnPair {
        server_conn,
        client_conn,
    }
}

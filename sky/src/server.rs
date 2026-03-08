use std::net::{IpAddr, Ipv6Addr, SocketAddr};

// stands for hermes sky
const PORT: u16 = u16::from_be_bytes(*b"hs");

use kademlia::Id;
use quinn::{
    RecvStream, SendStream, ServerConfig,
    rustls::pki_types::{CertificateDer, PrivateKeyDer},
};
use rustls::pki_types::PrivatePkcs8KeyDer;
use sha2::{
    Sha256,
    digest::{DynDigest, FixedOutput},
};

use tracing::warn;

use crate::{error::Error, get_public_ip};

pub struct Server {
    socket: quinn::Endpoint,
    id: Id<32>,
}

pub fn ip_to_id(ip: IpAddr) -> Id<32> {
    let mut hasher: Sha256 = sha2::Digest::new();
    hasher.update(b"hermes-sky");
    match ip {
        std::net::IpAddr::V4(ipv4_addr) => hasher.update(&ipv4_addr.octets()),
        std::net::IpAddr::V6(ipv6_addr) => hasher.update(&ipv6_addr.octets()),
    };
    let id = <[u8; 32]>::from(hasher.finalize_fixed()).into();
    id
}

struct RpcHandler {
    send_stream: SendStream,
    recv_stream: RecvStream,
}

impl From<(SendStream, RecvStream)> for RpcHandler {
    fn from((send_stream, recv_stream): (SendStream, RecvStream)) -> Self {
        Self {
            send_stream,
            recv_stream,
        }
    }
}

impl RpcHandler {
    pub async fn handle_rpc_call(mut self) -> Result<(), Error> {
        let Some(chunk) = self.recv_stream.read_chunk(2000, true).await? else {
            return Ok(());
        };
        let req: schema::sky_node::rpc::Request = minicbor::decode(&chunk.bytes)?;
        todo!();
        Ok(())
    }
}

impl Server {
    fn bind_with_cert(
        certs: Vec<CertificateDer<'static>>,
        priv_key: PrivateKeyDer<'static>,
        addr: SocketAddr,
    ) -> Result<Self, Error> {
        let id = ip_to_id(addr.ip());
        let endpoint =
            quinn::Endpoint::server(ServerConfig::with_single_cert(certs, priv_key)?, addr)?;
        Ok(Self {
            socket: endpoint,
            id,
        })
    }
    pub async fn start(self) {
        while let Some(next) = self.socket.accept().await {
            tokio::spawn(async move {
                let conn = match next.await {
                    Ok(res) => res,
                    Err(e) => {
                        warn!("incoming req failed: {e}");
                        return;
                    }
                };
                while let Ok(acc) = conn.accept_bi().await {}
            });
        }
    }
    pub async fn bind_with_self_signed() -> Result<Self, Error> {
        let public_ip = get_public_ip().await.ok_or(Error::NoPublicIp)?;
        let hashed_id = ip_to_id(public_ip);
        let id = hashed_id.to_string();

        let left = &id[..id.len() / 2];
        let right = &id[id.len() / 2..];

        let url = format!("{left}.{right}.invalid");

        let cert = rcgen::generate_simple_self_signed(vec![url])?;
        let cert_der = CertificateDer::from(cert.cert);
        let key = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
        Self::bind_with_cert(
            vec![cert_der],
            key.into(),
            SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), PORT),
        )
    }
}

use std::net::{IpAddr, SocketAddr};

// stands for hermes sky
const PORT: u16 = u16::from_be_bytes(*b"hs");

use kademlia::Id;
use quinn::{
    ServerConfig,
    rustls::pki_types::{CertificateDer, PrivateKeyDer},
};
use rustls::pki_types::PrivatePkcs8KeyDer;
use sha2::{
    Sha256,
    digest::{DynDigest, FixedOutput},
};

use crate::{error::Error, get_public_ip::get_public_ip};

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

impl Server {
    pub fn start_with_cert(
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

    pub fn start_with_self_signed(addr: SocketAddr) -> Result<Self, Error> {
        let hashed_id = ip_to_id(addr.ip());
        let id = hashed_id.to_string();

        let left = &id[..id.len() / 2];
        let right = &id[id.len() / 2..];

        let url = format!("{left}.{right}.invalid");

        let cert = rcgen::generate_simple_self_signed(vec![url])?;
        let cert_der = CertificateDer::from(cert.cert);
        let key = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
        Self::start_with_cert(vec![cert_der], key.into(), addr)
    }
}

pub(crate) async fn data_dir() -> std::path::PathBuf {
    let out = dirs::data_dir().unwrap().join("hermes");
    tokio::fs::create_dir_all(&out).await.unwrap();
    out
}

pub async fn regularly_refresh_cert() {}

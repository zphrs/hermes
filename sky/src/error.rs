use std::fmt::Display;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("rustls: {0}")]
    Rustls(#[from] rustls::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("rcgen: {0}")]
    Rcgen(#[from] rcgen::Error),
    #[error("rustls::pki_types::pem::Error")]
    PkiTypes(#[from] rustls::pki_types::pem::Error),
    #[error("couldn't resolve public ip")]
    NoPublicIp,
}

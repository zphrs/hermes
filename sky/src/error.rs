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
    #[error("quinn read: {0}")]
    QuinnRead(#[from] quinn::ReadError),
    #[error("quinn write: {0}")]
    QuinnWrite(#[from] quinn::WriteError),
    #[error("minicbor decode: {0}")]
    MinicborDecode(#[from] minicbor::decode::Error),
    #[error("minicbor encode: {0}")]
    MinicborEncode(String),
}

impl<T: Display> From<minicbor::encode::Error<T>> for Error {
    fn from(value: minicbor::encode::Error<T>) -> Self {
        Self::MinicborEncode(value.to_string())
    }
}

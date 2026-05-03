use thiserror::Error;

use crate::net::error::ParseError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to parse packet: {0}")]
    PacketParse(#[from] ParseError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid packet: {0}")]
    InvalidPacket(&'static str),
}

use thiserror::Error;

use crate::net::error::ParseError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to parse packet: {0}")]
    PacketParseError(#[from] ParseError),
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("invalid packet: {0}")]
    InvalidPacket(&'static str),
}

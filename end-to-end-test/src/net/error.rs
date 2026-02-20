#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    #[error("invalid IP version: {0}")]
    InvalidVersion(u8),
    #[error("invalid IHL: {0}; expected 5. Might indicate presence of options field")]
    InvalidIhl(u8),
    #[error("invalid protocol number: {0}")]
    InvalidProtocolNumber(u8),
    #[error("invalid checksum: {0}")]
    InvalidChecksum(u16),
    #[error("unexpected next header: {0}. Might indicate IPV6 extension")]
    UnexpectedNextHeader(u8),
    #[error("not enough bytes for a header. Expected {expected} and found {had}")]
    NotEnoughForHeaders { expected: u16, had: usize },
    #[error("not enough bytes for body content. Expected {expected} and found {had}")]
    NotEnoughForData { expected: u16, had: usize },
}

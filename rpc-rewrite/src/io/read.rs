//! reading a message from a stream follows these steps:
//! 1. receive message
//! 2. decode message

use minicbor::Decode;

use crate::{io::read_into_buf, traits::io::BytesReadStream};
#[derive(thiserror::Error)]
pub enum Error<RecvStream: BytesReadStream> {
    #[error(transparent)]
    Receive(RecvStream::Error),
    #[error("invalid data")]
    Decode(#[from] minicbor::decode::Error),
}

impl<RecvStream: BytesReadStream> std::fmt::Debug for Error<RecvStream> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Receive(arg0) => f.debug_tuple("Receive").field(arg0).finish(),
            Self::Decode(arg0) => f.debug_tuple("Decode").field(arg0).finish(),
        }
    }
}

pub async fn read<'buf, Message, RecvStream: BytesReadStream>(
    buf: &'buf mut Vec<u8>,
    recv: RecvStream,
) -> Result<Message, Error<RecvStream>>
where
    Message: Decode<'buf, ()>,
{
    read_into_buf(recv, buf, usize::MAX)
        .await
        .map_err(Error::Receive)?;
    Ok(minicbor::decode(buf)?)
}

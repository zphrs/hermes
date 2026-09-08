//! writing to a stream follows these steps:
//! 1. encode message
//! 2. send message

use std::convert::Infallible;

use bytes::Bytes;
use minicbor::{CborLen, Encode};

use super::BytesWriteStream;

#[derive(thiserror::Error)]
pub enum Error<S: BytesWriteStream> {
    #[error("could not encode: {0}")]
    Encode(#[from] minicbor::encode::Error<Infallible>),
    #[error(transparent)]
    Send(S::Error),
}

impl<S: BytesWriteStream> std::fmt::Debug for Error<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(arg0) => f.debug_tuple("Encode").field(arg0).finish(),
            Self::Send(arg0) => f.debug_tuple("Send").field(arg0).finish(),
        }
    }
}

/// moves send to allow for pinning and to allow for dropping `send` after
/// write_all succeeds
pub(crate) async fn bytes<B: BytesWriteStream>(
    mut send: B,
    buf: Bytes,
    assert_stopped: bool,
) -> Result<(), B::Error> {
    send.try_put(buf).await?;
    if assert_stopped {
        send.finish();
        send.stopped().await;
    }
    Ok(())
}

pub async fn write<Message: CborLen<()> + Encode<()>, SendStream: BytesWriteStream>(
    request: &Message,
    send: SendStream,
) -> Result<(), Error<SendStream>> {
    let mut buf = Vec::with_capacity(minicbor::len(request));
    minicbor::encode(request, &mut buf)?;
    bytes(send, Bytes::from(buf), core::cfg!(test))
        .await
        .map_err(Error::Send)?;
    Ok(())
}

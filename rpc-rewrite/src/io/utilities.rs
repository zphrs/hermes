use bytes::Bytes;

use crate::traits::io::{BytesReadStream, BytesWriteStream, Stopped};

/// will return if maxlen is hit or if the end of the stream is reached.
pub(crate) async fn read_to_end<B: BytesReadStream>(
    mut recv: B,
    buf: &mut Vec<u8>,
    maxlen: usize,
) -> Result<(), B::Error> {
    while let Some(bytes) = recv.try_next().await? {
        if bytes.len() + buf.len() >= maxlen {
            return Ok(());
        }
        buf.extend(bytes);
    }

    Ok(())
}

/// moves send to allow for pinning and to allow for dropping `send` after
/// write_all succeeds
pub(crate) async fn write_all<B: BytesWriteStream>(
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

use bytes::Bytes;

use crate::traits::io::{BytesReadStream, BytesWriteStream};

/// will return if maxlen is hit or if the end of the stream is reached.
pub(crate) async fn read_into_buf<B: BytesReadStream>(
    mut recv: B,
    buf: &mut impl for<'a> Extend<&'a u8>,
    max_length: usize,
) -> Result<(), B::Error> {
    let mut so_far = 0usize;
    while let Some(bytes) = recv.try_next().await? {
        if bytes.len() + so_far >= max_length {
            return Ok(());
        }
        buf.extend(bytes.iter());
        so_far += bytes.len();
    }

    Ok(())
}

/// moves send to allow for pinning and to allow for dropping `send` after
/// write_all succeeds
pub(crate) async fn write_bytes<B: BytesWriteStream>(
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

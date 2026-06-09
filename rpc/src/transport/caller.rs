use maxlen::MaxLen;
use tracing::debug;

use super::{BiStream, CallerError};
use crate::{Method, RpcError, RpcMessage};
use std::fmt::Debug;

pub trait Caller: Send + Sync + BiStream + Sized {
    type Error;
    fn open_stream(
        &self,
    ) -> impl Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::Error>> + Send;

    fn query<M: Method, Root: RpcMessage + From<M::Req> + Send + Debug>(
        &self,
        req: M::Req,
    ) -> impl Future<Output = Result<M::Res, CallerError<Self::Error>>> + Send
    where
        M::Res: RpcMessage,
    {
        async {
            let (write, read) = self.open_stream().await.map_err(CallerError::Transport)?;
            debug!("sending query");

            {
                let root: Root = req.into();
                let mut sender = minicbor_io::AsyncWriter::new(write);
                sender.write(root).await.map_err(RpcError::from)?;
                // drops write here to indicate no more writes will occur
            }
            debug!("sent query");

            let mut receiver = minicbor_io::AsyncReader::new(read);

            receiver.set_max_len(<M::Res as MaxLen>::max_len() as u32);
            let out = receiver
                .read::<M::Res>()
                .await
                .map_err(RpcError::from)?
                .ok_or(RpcError::Closed)?;
            debug!("received message");
            Ok(out)
        }
    }
}

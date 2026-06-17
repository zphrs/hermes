use maxlen::MaxLen;
use tracing::debug;

use super::{BiStream, CallerError};
use crate::{Method, RpcMessage, traits::method::not_applicable::NotApplicable};

pub trait Caller: BiStream + Sized {
    type Error;
    fn open_stream(
        &self,
    ) -> impl Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::Error>>;

    fn query<M: Method, RootReq: RpcMessage>(
        &self,
        req: M::Req,
    ) -> impl Future<Output = Result<M::Res, CallerError<Self::Error>>>
    where
        RootReq: From<M::Req>,
        M::Res: RpcMessage,
    {
        async {
            let (write, read) = self.open_stream().await.map_err(CallerError::Transport)?;
            debug!("sending query");

            {
                let root: RootReq = req.into();
                let mut sender = minicbor_io::AsyncWriter::new(write);
                sender.write(root).await.map_err(CallerError::Minicbor)?;
                // drops write here to indicate no more writes will occur
            }
            debug!("sent query");

            let mut receiver = minicbor_io::AsyncReader::new(read);

            receiver.set_max_len(<M::Res as MaxLen>::max_len() as u32);
            let out = receiver
                .read::<M::Res>()
                .await
                .map_err(CallerError::Minicbor)?
                .ok_or(CallerError::Closed)?;
            debug!("received message");
            Ok(out)
        }
    }

    fn notify<M: Method<Res = NotApplicable>, RootReq: RpcMessage>(
        &self,
        req: M::Req,
    ) -> impl Future<Output = Result<(), CallerError<Self::Error>>>
    where
        RootReq: From<M::Req>,
    {
        async {
            let (write, _read) = self.open_stream().await.map_err(CallerError::Transport)?;
            debug!("sending notification");

            {
                let root: RootReq = req.into();
                let mut sender = minicbor_io::AsyncWriter::new(write);
                sender.write(root).await.map_err(CallerError::Minicbor)?;
                // drops write here to indicate no more writes will occur
            }
            debug!("sent notification");

            Ok(())
        }
    }
}

use maxlen::MaxLen;
use tracing::debug;

use super::{BiStream, CallerError};
use crate::{
    Method, RpcError, RpcMessage, StreamMethod,
    transport::{Close, streams::InitializedMessageStream},
};
use std::fmt::Debug;

pub trait Caller: Send + Sync + BiStream + Sized {
    type Error;
    fn open_stream(
        &self,
    ) -> impl Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::Error>> + Send;

    fn query<M: Method, Root: RpcMessage + From<M::Req> + Send + Debug>(
        &self,
        req: M::Req,
    ) -> impl Future<Output = Result<M::Res, CallerError<Self::Error>>> + Send {
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

    fn init_message_stream<M: StreamMethod, Root: RpcMessage + From<<M as Method>::Req> + Send>(
        &self,
        req: <M as Method>::Req,
    ) -> impl Future<Output = Result<InitializedMessageStream<Self>, CallerError<Self::Error>>> + Send
    {
        async {
            let (mut write, read) = self.open_stream().await.map_err(CallerError::Transport)?;

            let root = Root::from(req);
            assert!(root.cbor_len(&mut ()) <= Root::max_len());
            let mut sender = minicbor_io::AsyncWriter::new(&mut write);
            sender.write(root).await.map_err(RpcError::from)?;
            Ok(InitializedMessageStream::new((write, read).into()))
        }
    }
    // just dropping it should close the stream
    fn close_message_stream<M: StreamMethod>(
        &self,
        msg_stream: InitializedMessageStream<Self>,
    ) -> impl Future<Output = ()> + Send
    where
        Self::SendStream: Close,
    {
        async { msg_stream.close().await }
    }

    fn send_to_message_stream<M: StreamMethod>(
        &self,
        req: <M as StreamMethod>::Req,
        msg_stream: &mut InitializedMessageStream<Self>,
    ) -> impl Future<Output = Result<(), CallerError<Self::Error>>> + Send {
        async {
            let mut sender = minicbor_io::AsyncWriter::new(msg_stream.write_mut());
            sender.write(req).await.map_err(RpcError::from)?;
            Ok(())
        }
    }

    fn query_from_stream<M: StreamMethod>(
        &self,
        streams: &mut InitializedMessageStream<Self>,
    ) -> impl Future<Output = Result<Option<<M as StreamMethod>::Res>, CallerError<Self::Error>>> + Send
    {
        async {
            let mut receiver = minicbor_io::AsyncReader::new(streams.read_mut());
            Ok(receiver
                .read::<<M as StreamMethod>::Res>()
                .await
                .map_err(RpcError::from)?)
        }
    }
}

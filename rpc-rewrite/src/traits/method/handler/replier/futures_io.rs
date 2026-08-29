use std::{convert::Infallible, marker::PhantomData, pin::pin};

use futures::{AsyncWriteExt, io::AsyncWrite};
use minicbor::CborLen;

use crate::traits::method;

pub struct Replier<'buffer, M: method::Branch, SendStream: AsyncWrite> {
    stream: SendStream,
    buffer: &'buffer mut Vec<u8>,
    _marker: PhantomData<M>,
}

impl<'buf, M: method::Branch, SendStream: AsyncWrite> Replier<'buf, M, SendStream> {
    pub fn new_with_buffer(stream: SendStream, buffer: &'buf mut Vec<u8>) -> Self {
        Self {
            stream,
            buffer,
            _marker: PhantomData,
        }
    }

    pub fn map<Descendant: method::Branch>(self) -> Replier<'buf, Descendant, SendStream> {
        let Self { stream, buffer, .. } = self;
        Replier {
            stream,
            buffer,
            _marker: PhantomData,
        }
    }
}

pub struct Receipt<Res>(Res);

impl<Res> super::Receipt<Res> for Receipt<Res> {
    type Error = Infallible;

    async fn finalize(self) -> Result<Res, Self::Error> {
        Ok(self.0)
    }
}

impl<Res> Receipt<Res> {
    fn map<T>(self, mapper: impl FnOnce(Res) -> T) -> Receipt<T> {
        let Self(res) = self;
        Receipt(mapper(res))
    }
}

impl<'a, M: method::Branch, SendStream: AsyncWrite> super::Replier<M>
    for Replier<'a, M, SendStream>
{
    type Receipt<'buf, Res> = Receipt<Res>;
    type Error = std::io::Error;

    async fn reply_with_branch<
        'buf,
        Descendant: method::Branch + method::Descendant<M>,
        DescendantHandler: method::handler::BranchHandler<Descendant>,
    >(
        self,
        request: Descendant::Req<'buf>,
        handler: &mut DescendantHandler,
    ) -> Result<Self::Receipt<'buf, <M>::Res<'buf>>, Self::Error> {
        let mapped_replier = self.map();
        let res = handler.handle(request, mapped_replier).await?;
        Ok(res.map(Descendant::res_to_parent))
    }

    async fn reply_with_leaf<
        'buf,
        Descendant: method::Leaf + method::Descendant<M>,
        DescendantHandler: method::handler::LeafHandler<Descendant>,
    >(
        mut self,
        request: Descendant::Req<'buf>,
        handler: &mut DescendantHandler,
    ) -> Result<Self::Receipt<'buf, <M>::Res<'buf>>, Self::Error>
    where
        Descendant::Res<'buf>: minicbor::Encode<()> + minicbor::CborLen<()>,
    {
        let res = handler.handle(request).await;
        self.buffer.clear();
        let len = res.cbor_len(&mut ());
        self.buffer.reserve(len);

        minicbor::encode(&res, &mut self.buffer).unwrap();
        pin!(self.stream).write_all(self.buffer).await?;
        Ok(Receipt(Descendant::res_to_parent(res)))
    }
}

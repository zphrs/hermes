use std::{convert::Infallible, marker::PhantomData, pin::pin};

use futures::io::AsyncWrite;
use minicbor::CborLen;

use crate::traits::{
    io::BytesWriteStream,
    method::{self, ResOf},
};

pub struct Replier<M: method::Branch, SendStream: BytesWriteStream> {
    stream: SendStream,
    _marker: PhantomData<M>,
}

impl<'buf, M: method::Branch, SendStream: BytesWriteStream> Replier<M, SendStream> {
    pub fn new(stream: SendStream) -> Self {
        Self {
            stream,
            _marker: PhantomData,
        }
    }

    pub fn map<Descendant: method::Branch>(self) -> Replier<Descendant, SendStream> {
        let Self { stream, .. } = self;
        Replier {
            stream,
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

impl<M: method::Branch, SendStream: BytesWriteStream> super::Replier<M> for Replier<M, SendStream> {
    type Receipt<Res> = Receipt<Res>;
    type Error = SendStream::Error;

    async fn reply_with_branch<
        'buf,
        Descendant: method::Branch + method::Descendant<M>,
        DescendantHandler: method::handler::BranchHandler<Descendant>,
    >(
        self,
        request: method::ReqOf<'buf, Descendant>,
        handler: &mut DescendantHandler,
    ) -> Result<Self::Receipt<method::ResOf<'buf, M>>, Self::Error> {
        let mapped_replier: Replier<Descendant, SendStream> = self.map();
        let res: Receipt<_> = handler.handle(request, mapped_replier).await?;
        Ok(res.map(Descendant::res_to_parent))
    }

    async fn reply_with_leaf<
        'buf,
        Descendant: method::Leaf + method::Descendant<M> + method::Loopback,
        DescendantHandler: method::handler::LeafHandler<Descendant>,
    >(
        mut self,
        request: method::ReqOf<'buf, Descendant>,
        handler: &mut DescendantHandler,
    ) -> Result<Self::Receipt<ResOf<'buf, M>>, Self::Error>
    where
        ResOf<'buf, Descendant>: minicbor::Encode<()> + minicbor::CborLen<()>,
    {
        let res = handler.handle(request).await;

        let mut buffer = Vec::with_capacity(res.cbor_len(&mut ()));

        minicbor::encode(&res, &mut buffer).unwrap();
        self.stream.try_put(buffer.into()).await?;
        Ok(Receipt(Descendant::res_to_parent(res)))
    }
}

use std::marker::PhantomData;

use crate::Method;
use crate::io::BytesWriteStream;

use crate::method::{self, ResOf};

pub struct Replier<M, SendStream: BytesWriteStream> {
    stream: SendStream,
    _marker: PhantomData<M>,
}

impl<M, SendStream: BytesWriteStream> Replier<M, SendStream> {
    pub fn new(stream: SendStream) -> Self {
        Self {
            stream,
            _marker: PhantomData,
        }
    }

    pub fn map<Descendant>(self) -> Replier<Descendant, SendStream> {
        let Self { stream, .. } = self;
        Replier {
            stream,
            _marker: PhantomData,
        }
    }
}

mod receipt;

pub use receipt::Receipt;

impl<M: Method, SendStream: BytesWriteStream> super::Replier<M> for Replier<M, SendStream> {
    type Receipt<Res> = Receipt<Res>;
    type Error = crate::io::write::Error<SendStream>;

    async fn reply_with_branch<
        'req,
        BT: crate::marker::BranchType,
        Descendant: method::OfType<method::Branch<BT>> + method::Descendant<M>,
        DescendantHandler: method::handler::BranchHandler<Descendant>,
    >(
        self,
        request: method::ReqOf<'req, Descendant>,
        handler: &mut DescendantHandler,
    ) -> Result<Self::Receipt<ResOf<'req, M>>, Self::Error> {
        let mapped_replier: Replier<Descendant, SendStream> = self.map();
        let res: Receipt<_> = handler.handle(request, mapped_replier).await?;
        Ok(res.map(Descendant::res_to_parent))
    }
}

impl<M: Method, SendStream: BytesWriteStream> super::loopback::Replier<M>
    for Replier<M, SendStream>
{
    async fn reply_with_leaf<
        'req,
        Descendant: method::OfType<method::LeafLoopback> + method::Descendant<M>,
        DescendantHandler: method::handler::LeafHandler<Descendant>,
    >(
        self,
        request: method::ReqOf<'req, Descendant>,
        handler: &mut DescendantHandler,
    ) -> Result<Receipt<ResOf<'req, M>>, Self::Error>
    where
        ResOf<'req, Descendant>: minicbor::Encode<()> + minicbor::CborLen<()>,
    {
        let res = handler.handle(request).await;

        crate::io::write::write(&res, self.stream).await?;

        Ok(Receipt(Descendant::res_to_parent(res)))
    }
}

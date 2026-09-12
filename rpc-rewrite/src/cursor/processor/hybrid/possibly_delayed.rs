use std::marker::PhantomData;

use crate::{
    Method,
    cursor::{processor::transition::delayed_replier, state::WrapperCredit},
    io::{BytesWriteStream, write},
    method::replier::{self, immediate},
};

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
        Replier::new(self.stream)
    }
}

pub enum Receipt<Res, SendStream: BytesWriteStream> {
    Delayed(delayed_replier::Receipt<Res, SendStream>),
    Immediate(immediate::Receipt<Res>),
}

impl<Res, SendStream: BytesWriteStream> Receipt<Res, SendStream> {
    pub fn map<T>(self, mapper: impl FnOnce(Res) -> T) -> Receipt<T, SendStream> {
        match self {
            Receipt::Delayed(receipt) => Receipt::Delayed(receipt.map(mapper)),
            Receipt::Immediate(receipt) => Receipt::Immediate(receipt.map(mapper)),
        }
    }
}

impl<Res, SendStream: BytesWriteStream> replier::Receipt<Res> for Receipt<Res, SendStream> {
    type Error = <delayed_replier::Receipt<Res, SendStream> as replier::Receipt<Res>>::Error;

    async fn finalize(self) -> Result<Res, Self::Error> {
        let res = match self {
            Receipt::Delayed(receipt) => receipt.finalize().await?,
            Receipt::Immediate(receipt) => match receipt.finalize().await {
                Ok(v) => v,
                Err(e) => match e {},
            },
        };
        Ok(res)
    }
}

impl<M: Method, SendStream: BytesWriteStream> replier::Replier<M> for Replier<M, SendStream> {
    type Receipt<Res> = Receipt<Res, SendStream>;

    type Error = write::Error<SendStream>;

    async fn reply_with_branch<
        'req,
        BT: crate::marker::BranchType,
        Descendant: crate::method::OfType<crate::method::Branch<BT>> + crate::method::Descendant<M>,
        DescendantHandler: crate::method::handler::BranchHandler<Descendant>,
    >(
        self,
        request: crate::method::ReqOf<'req, Descendant>,
        handler: &mut DescendantHandler,
    ) -> Result<Self::Receipt<crate::method::ResOf<'req, M>>, Self::Error> {
        handler
            .handle(request, self.map())
            .await
            .map(|v| v.map(Descendant::res_to_parent))
    }
}

impl<M: Method, SendStream: BytesWriteStream> replier::loopback::Replier<M>
    for Replier<M, SendStream>
{
    async fn reply_with_leaf<
        'req,
        Descendant: crate::method::OfType<crate::method::LeafLoopback> + crate::method::Descendant<M>,
        DescendantHandler: crate::method::handler::LeafHandler<Descendant>,
    >(
        self,
        request: crate::method::ReqOf<'req, Descendant>,
        handler: &mut DescendantHandler,
    ) -> Result<Self::Receipt<crate::method::ResOf<'req, M>>, Self::Error>
    where
        crate::method::ResOf<'req, Descendant>: minicbor::Encode<()> + minicbor::CborLen<()>,
    {
        let res = handler.handle(request).await;

        crate::io::write::write(&res, self.stream).await?;

        Ok(Receipt::Immediate(immediate::Receipt(
            Descendant::res_to_parent(res),
        )))
    }
}

impl<M: Method, SendStream: BytesWriteStream> replier::transition::Replier<M>
    for Replier<M, SendStream>
{
    async fn transition_with_leaf<
        'req,
        Descendant: crate::method::OfType<crate::method::LeafTransition> + crate::method::Descendant<M>,
        DescendantHandler: crate::method::handler::transition::LeafHandler<Descendant>,
    >(
        self,
        request: crate::method::ReqOf<'req, Descendant>,
        handler: DescendantHandler,
    ) -> Result<
        (
            Self::Receipt<crate::method::ResOf<'req, M>>,
            DescendantHandler::NextHandler,
        ),
        Self::Error,
    >
    where
        crate::method::ResOf<'req, Descendant>: minicbor::Encode<()> + minicbor::CborLen<()>,
    {
        let res = handler
            .handle_transition(request, WrapperCredit::new())
            .await;

        let receipt =
            delayed_replier::Receipt::new(self.stream, res.0, true)?.map(Descendant::res_to_parent);

        Ok((Receipt::Delayed(receipt), res.1))
    }
}

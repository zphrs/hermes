use std::{convert::Infallible, marker::PhantomData};

use crate::{
    io::write_all,
    traits::{
        self,
        handler::BranchHandler,
        io::BytesWriteStream,
        method::{self, ResOf, handler},
        replier,
        state::WrapperCredit,
    },
};

#[derive(minicbor::CborLen, minicbor::Encode, minicbor::Decode)]
pub struct TransitionReply<T> {
    #[n(0)]
    pub reply: T,
    #[n(1)]
    pub in_tiebreak: bool,
}

impl<T> TransitionReply<T> {
    /// Creates a new [`TransitionReply<T>`].
    pub fn new(reply: T) -> Self {
        Self {
            reply,
            in_tiebreak: false,
        }
    }

    /// Sets the in tiebreak of this [`TransitionReply<T>`].
    ///
    /// # Panics
    ///
    /// Panics if the serialization format has changed on TransitionReply
    /// (e.g. if a third field was added to the struct).
    pub fn set_in_tiebreak(encoded: &mut [u8], in_transition: bool) {
        let bool_position = {
            let mut decoder = minicbor::Decoder::new(encoded);
            let len = decoder.array().unwrap().unwrap();
            assert_eq!(len, 2);
            decoder.skip().unwrap();
            decoder.position()
        };
        let mut encoder = minicbor::Encoder::new(&mut encoded[bool_position..]);
        encoder.bool(in_transition).unwrap();
    }
}

pub(super) struct DelayedReplier<M: method::Branch, SendStream: BytesWriteStream> {
    stream: SendStream,
    _marker: PhantomData<M>,
}

impl<M: method::Branch, SendStream: BytesWriteStream> DelayedReplier<M, SendStream> {
    pub fn new(stream: SendStream) -> Self {
        Self {
            stream,

            _marker: PhantomData,
        }
    }

    pub fn map<Descendant: method::Branch>(self) -> DelayedReplier<Descendant, SendStream> {
        let Self { stream, .. } = self;
        DelayedReplier {
            stream,
            _marker: PhantomData,
        }
    }
}

impl<M: method::Branch, SendStream: BytesWriteStream> traits::Replier<M>
    for DelayedReplier<M, SendStream>
{
    type Receipt<Res> = Receipt<Res, SendStream>;

    type Error = minicbor::encode::Error<Infallible>;

    async fn reply_with_branch<
        'buf,
        Descendant: method::Branch + method::Descendant<M>,
        DescendantHandler: BranchHandler<Descendant>,
    >(
        self,
        request: method::ReqOf<'buf, Descendant>,
        handler: &mut DescendantHandler,
    ) -> Result<Self::Receipt<ResOf<'buf, M>>, Self::Error> {
        let mapped_replier = self.map();
        let res: Receipt<_, _> = handler.handle(request, mapped_replier).await?;
        Ok(res.map(Descendant::res_to_parent))
    }

    async fn reply_with_leaf<
        'buf,
        Descendant: method::Leaf + method::Descendant<M> + method::Loopback,
        DescendantHandler: handler::LeafHandler<Descendant>,
    >(
        self,
        request: method::ReqOf<'buf, Descendant>,
        handler: &mut DescendantHandler,
    ) -> Result<Self::Receipt<ResOf<'buf, M>>, Self::Error>
    where
        ResOf<'buf, Descendant>: minicbor::Encode<()> + minicbor::CborLen<()>,
    {
        let res = handler.handle(request).await;

        let receipt = Receipt::new(self.stream, res, true)?.map(Descendant::res_to_parent);
        Ok(receipt)
    }
}
impl<M: method::Branch + method::Transitions, SendStream: BytesWriteStream>
    replier::transition::Replier<M> for DelayedReplier<M, SendStream>
{
    async fn reply_with_leaf<
        'buf,
        Descendant: method::Leaf + method::Descendant<M> + method::Transitions,
        DescendantHandler: method::handler::transition::LeafHandler<Descendant>,
    >(
        self,
        request: method::ReqOf<'buf, Descendant>,
        mut handler: DescendantHandler,
    ) -> Result<
        (
            Self::Receipt<ResOf<'buf, M>>,
            DescendantHandler::NextHandler,
        ),
        Self::Error,
    >
    where
        ResOf<'buf, Descendant>: minicbor::Encode<()> + minicbor::CborLen<()>,
    {
        let res = handler
            .handle_transition(request, WrapperCredit::new())
            .await;

        let receipt = Receipt::new(self.stream, res.0, true)?.map(Descendant::res_to_parent);
        Ok((receipt, res.1))
    }
}

pub struct Receipt<Res, SendStream: BytesWriteStream> {
    stream: SendStream,
    buffer: Vec<u8>,
    in_tiebreak: Option<bool>,
    res: Res,
}

impl<Res, SendStream: BytesWriteStream> Receipt<Res, SendStream> {
    fn new(
        stream: SendStream,
        res: Res,
        in_transition: bool,
    ) -> Result<Self, minicbor::encode::Error<Infallible>>
    where
        Res: minicbor::Encode<()> + minicbor::CborLen<()>,
    {
        if in_transition {
            let transition_reply = TransitionReply::new(res);
            let mut buffer = Vec::with_capacity(minicbor::len(&transition_reply));
            minicbor::encode(&transition_reply, &mut buffer)?;
            Ok(Self {
                stream,
                buffer,
                in_tiebreak: Some(false),
                res: transition_reply.reply,
            })
        } else {
            let mut buffer = Vec::with_capacity(minicbor::len(&res));
            minicbor::encode(&res, &mut buffer)?;
            Ok(Self {
                stream,
                buffer,
                in_tiebreak: None,
                res,
            })
        }
    }

    fn map<T>(self, mapper: impl FnOnce(Res) -> T) -> Receipt<T, SendStream> {
        let Self {
            stream,
            buffer,
            in_tiebreak,
            res,
        } = self;
        Receipt {
            stream,
            buffer,
            in_tiebreak,
            res: mapper(res),
        }
    }

    pub(crate) fn res(&self) -> &Res {
        &self.res
    }

    /// Sets the in tiebreak of this [`Receipt<Res, SendStream>`].
    ///
    /// [`in_tiebreak`](Self::in_tiebreak) defaults to false in a transition.
    /// Outside of a transition in_tiebreak is not applicable.
    ///
    /// # Panics
    ///
    /// Panics if called outside of a transition.
    pub(crate) fn set_in_tiebreak(&mut self, in_tiebreak: bool) {
        let existing_in_tiebreak = self
            .in_tiebreak
            .expect("set_in_tiebreak should only be called during a transition");
        if existing_in_tiebreak != in_tiebreak {
            self.in_tiebreak = Some(in_tiebreak)
        }
    }
}

impl<Res, SendStream: BytesWriteStream> traits::Receipt<Res> for Receipt<Res, SendStream> {
    type Error = SendStream::Error;

    async fn finalize(mut self) -> Result<Res, Self::Error> {
        if let Some(in_tiebreak) = self.in_tiebreak {
            TransitionReply::<Infallible>::set_in_tiebreak(self.buffer.as_mut_slice(), in_tiebreak);
        }
        write_all(self.stream, self.buffer.into(), false).await?;
        Ok(self.res)
    }
}

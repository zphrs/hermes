use std::marker::PhantomData;

use maxlen::MaxLen;

use crate::traits::method::can_transition;

pub struct TransitionRequestMethod<'a, M: crate::Method, H: crate::Handler<M>> {
    handler: &'a mut H,
    in_tiebreak: bool,
    marker: PhantomData<M>,
}

impl<'a, M: crate::Method, H: crate::Handler<M>> TransitionRequestMethod<'a, M, H> {
    pub fn new(handler: &'a mut H, flag: bool) -> Self {
        Self {
            handler: handler,
            in_tiebreak: flag,
            marker: PhantomData,
        }
    }
}

/// Sets the tiebreak boolean flag
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct Res<Inner>
where
    Inner: crate::RpcMessage,
{
    #[n(0)]
    in_tiebreak: bool,
    #[n(1)]
    res: Inner,
}

impl<Inner> Res<Inner>
where
    Inner: crate::RpcMessage,
{
    fn new(res: Inner, in_tiebreak: bool) -> Self {
        Self { in_tiebreak, res }
    }

    pub fn into_parts(self) -> (bool, Inner) {
        (self.in_tiebreak, self.res)
    }
}

impl<'a, M: crate::Method, H: crate::Handler<M>> crate::Method for TransitionRequestMethod<'a, M, H>
where
    M::Req: crate::RpcMessage,
    M::Res: crate::RpcMessage,
{
    type Req = M::Req;

    type Res = Res<M::Res>;

    type CanTransition = can_transition::True;
}

impl<'a, M: crate::Method, H: crate::Handler<M>> crate::Handler
    for TransitionRequestMethod<'a, M, H>
where
    M::Req: crate::RpcMessage,
    M::Res: crate::RpcMessage,
{
    type Error = H::Error;

    async fn handle<Replier: crate::transport::ReplyHelper<Self>>(
        &mut self,
        replier: Replier,
        value: <Self as crate::Method>::Req,
    ) -> Result<
        <Replier as crate::transport::ReplyHelper<Self>>::Receipt<Self>,
        crate::traits::HandleError<
            <Replier as crate::transport::ReplyHelper<Self>>::Error,
            <Self as crate::Handler<Self>>::Error,
        >,
    > {
        replier
            .reply_with(self.handler, value, |v| Res::new(v, self.in_tiebreak))
            .await
    }
}

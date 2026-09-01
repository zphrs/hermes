use crate::traits::{
    Method, Replier,
    method::{ReqOf, ResOf},
};

/// intentionally impossible to construct so that it can serve
/// as a marker trait whenever a [Method], [Req](Method::Req), or [Res](Method::Res) should be
/// unset.
pub enum NotApplicable {}

impl<C> minicbor::CborLen<C> for NotApplicable {
    fn cbor_len(&self, _ctx: &mut C) -> usize {
        0
    }
}

impl<'b, C> minicbor::Decode<'b, C> for NotApplicable {
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor::decode::Error> {
        Err(minicbor::decode::Error::message(
            "NotApplicable is intentionally impossible to construct",
        )
        .at(d.position()))
    }
}

impl<C> minicbor::Encode<C> for NotApplicable {
    fn encode<W: minicbor::encode::Write>(
        &self,
        _e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        match *self {}
    }
}

impl Method for NotApplicable {
    type Req<'buf> = NotApplicable;

    type Res<'buf> = NotApplicable;

    type Transitions = super::False;

    type HasDescendants = super::True;
}

/// for when you need a handler that will never actually be invoked
/// (as the request type can never be constructed)
pub struct Handler;

impl crate::traits::BranchHandler<NotApplicable> for Handler {
    async fn handle<'a, R: Replier<NotApplicable>>(
        &mut self,
        request: ReqOf<'a, NotApplicable>,
        _replier: R,
    ) -> Result<R::Receipt<ResOf<'a, NotApplicable>>, R::Error> {
        match request {}
    }
}

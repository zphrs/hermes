use std::marker::PhantomData;

use crate::traits::{
    Method,
    method::{self, ResOf},
};

pub trait State {
    type ClientHandles: super::method::Branch;

    type ServerHandles: super::method::Branch;
}

pub struct Wrapper<S: State>(PhantomData<S>);

impl<S: State, C> minicbor::CborLen<C> for Wrapper<S> {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        self.0.cbor_len(ctx)
    }
}

impl<'b, S: State, C> minicbor::Decode<'b, C> for Wrapper<S> {
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor::decode::Error> {
        Ok(Self(d.decode()?))
    }
}

impl<S: State, C> minicbor::Encode<C> for Wrapper<S> {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.encode(self.0)?;
        Ok(())
    }
}

impl<S: State> Has<S> for Wrapper<S> {
    fn try_extract_wrapper(self) -> Result<Wrapper<S>, Self> {
        Ok(self)
    }
}

impl<M: method::Transitions + method::Leaf, S: State> From<WrapperCredit<M>> for Wrapper<S>
where
    for<'a> ResOf<'a, M>: Has<S>,
{
    fn from(value: WrapperCredit<M>) -> Self {
        Self::from_wrapper_credit(value)
    }
}

impl<S: State> Wrapper<S> {
    pub(crate) const fn new() -> Self {
        Self(PhantomData)
    }

    pub const fn from_wrapper_credit<M: method::Transitions + method::Leaf>(
        _credit: WrapperCredit<M>,
    ) -> Self
    where
        for<'a> ResOf<'a, M>: Has<S>,
    {
        Self::new()
    }
}

pub trait Entrypoint: State {}

pub trait Has<S: State>: Sized {
    fn try_extract_wrapper(self) -> Result<Wrapper<S>, Self>;
}

/// permission to create a new [`state::Wrapper`](Wrapper).
pub struct WrapperCredit<M: Method>(PhantomData<M>);

impl<M: Method> WrapperCredit<M> {
    pub(crate) fn new() -> Self {
        Self(PhantomData)
    }
}

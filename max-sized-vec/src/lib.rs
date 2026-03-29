use arrayvec::ArrayVec;
use maxlen::MaxLen;
use minicbor::{CborLen, Decode};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct MaxSizedVec<T, const N: usize>(arrayvec::ArrayVec<T, N>);

impl<T, const N: usize> MaxSizedVec<T, N> {
    pub fn inner(&self) -> &arrayvec::ArrayVec<T, N> {
        &self.0
    }

    pub fn into_inner(self) -> arrayvec::ArrayVec<T, N> {
        self.0
    }
}

impl<T, const N: usize> FromIterator<T> for MaxSizedVec<T, N> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self(ArrayVec::from_iter(iter))
    }
}

impl<C, T: CborLen<C>, const N: usize> CborLen<C> for MaxSizedVec<T, N> {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        N.cbor_len(ctx) + self.0.iter().map(|v| v.cbor_len(ctx)).sum::<usize>()
    }
}

impl<T: CborLen<()> + MaxLen, const N: usize> MaxLen for MaxSizedVec<T, N> {
    fn biggest_instantiation() -> Self {
        Self(ArrayVec::from_iter(
            (0..N).into_iter().map(|_| T::biggest_instantiation()),
        ))
    }
}

impl<'b, Ctx, T: Decode<'b, Ctx>, const N: usize> minicbor::Decode<'b, Ctx> for MaxSizedVec<T, N> {
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        ctx: &mut Ctx,
    ) -> Result<Self, minicbor::decode::Error> {
        let iter = d.array_iter_with(ctx)?;
        let len = iter.size_hint().1.ok_or(minicbor::decode::Error::message(
            "array must have a definite number of elements",
        ))?;
        if len > N {
            return Err(minicbor::decode::Error::message(
                "array overflowed max length",
            ));
        }
        let mut items = ArrayVec::new();
        for value in iter {
            items.push(value?)
        }

        Ok(Self(items))
    }
}

impl<T, Ctx, const N: usize> minicbor::Encode<Ctx> for MaxSizedVec<T, N>
where
    T: minicbor::Encode<Ctx>,
{
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.array(self.0.len() as u64)?;
        for item in &self.0 {
            item.encode(e, ctx)?;
        }
        Ok(())
    }
}

use std::ops::Deref;

use maxlen::MaxLen;
use minicbor::{self, CborLen};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Hash)]
pub struct MaxSizedString<const N: usize>(String);

impl<const N: usize> MaxSizedString<N> {
    pub fn inner(&self) -> &String {
        &self.0
    }

    /// # Errors
    ///
    /// If the length of the string is greater than the max length
    pub fn try_from_string(inner: String) -> Result<Self, String> {
        if inner.len() < N {
            Ok(MaxSizedString(inner))
        } else {
            Err(inner)
        }
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl<const N: usize> Deref for MaxSizedString<N> {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<C, const N: usize> CborLen<C> for MaxSizedString<N> {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        self.0.cbor_len(ctx)
    }
}

impl<const N: usize> MaxLen for MaxSizedString<N> {
    fn biggest_instantiation() -> Self {
        Self(
            // SAFETY: an array of tilde ASCII characters is valid UTF-8
            unsafe { String::from_utf8_unchecked((0..N).map(|_| b'~').collect()) },
        )
    }
}

impl<'b, Ctx, const N: usize> minicbor::Decode<'b, Ctx> for MaxSizedString<N> {
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        _ctx: &mut Ctx,
    ) -> Result<Self, minicbor::decode::Error> {
        let str = d.str()?;
        if str.len() > N {
            Err(minicbor::decode::Error::message("MaxSizedString too long").at(d.position()))?
        }
        Ok(MaxSizedString(str.into()))
    }
}

impl<Ctx, const N: usize> minicbor::Encode<Ctx> for MaxSizedString<N> {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut Ctx,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.str(&self.0)?;
        Ok(())
    }
}

impl<const N: usize> TryFrom<&str> for MaxSizedString<N> {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from_string(value.into())
    }
}

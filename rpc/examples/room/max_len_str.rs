use std::{fmt::Display, ops::Deref, sync::Arc};

use maxlen::MaxLen;

/// Wrapper around Arc<str> to allow for a cheaply cloneable string
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MaxLenStr<const MAX_LEN: usize>(Arc<str>);

impl<const MAX_LEN: usize> Display for MaxLenStr<MAX_LEN> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<const MAX_LEN: usize> TryFrom<&str> for MaxLenStr<MAX_LEN> {
    type Error = &'static str;
    #[inline]
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl<const MAX_LEN: usize> MaxLenStr<MAX_LEN> {
    /// # Errors Errors with "MaxSizedString too long" if the string length
    /// exceeds the MAX_LEN in terms of byte length (not Unicode length)
    pub fn try_new(str: impl Into<Arc<str>>) -> Result<Self, &'static str> {
        let str = str.into();
        if str.len() > MAX_LEN {
            Err("MaxSizedString too long")?
        }
        Ok(Self(str))
    }
}

impl<C, const MAX_LEN: usize> minicbor::Encode<C> for MaxLenStr<MAX_LEN> {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.str(&*self.0)?;
        Ok(())
    }
}

impl<'b, C, const MAX_LEN: usize> minicbor::Decode<'b, C> for MaxLenStr<MAX_LEN> {
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor::decode::Error> {
        let str = d.str()?;
        if str.len() > MAX_LEN {
            return Err(
                minicbor::decode::Error::message("MaxSizedString too long").at(d.position())
            );
        }
        Ok(Self(str.into()))
    }
}

impl<C, const MAX_LEN: usize> minicbor::CborLen<C> for MaxLenStr<MAX_LEN> {
    fn cbor_len(&self, ctx: &mut C) -> usize {
        self.0.cbor_len(ctx)
    }
}

impl<const MAX_LEN: usize> MaxLen for MaxLenStr<MAX_LEN> {
    fn biggest_instantiation() -> Self {
        let string = String::from_utf8_lossy(&[b'~'; MAX_LEN]);
        Self::try_new(string).unwrap()
    }
}

impl<const MAX_LEN: usize> Deref for MaxLenStr<MAX_LEN> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

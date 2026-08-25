use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
};

pub mod int_or_str;
use int_or_str::IntOrStr;
use minicbor::encode;

///
///
/// ```cddl
/// header_map = {
///     Generic_Headers,
///     * label => values
/// }
///
/// Generic_Headers = (
///     ? 1 => int / tstr,  ; algorithm identifier
///     ? 2 => [+label],    ; criticality
///     ? 3 => tstr / int,  ; content type
///     ? 4 => bstr,        ; key identifier
///     ? ( 5 => bstr //    ; IV
///         6 => bstr )     ; Partial IV
/// )
/// ```
pub struct HeaderMap<'de> {
    /// algorithm identifier
    pub alg: Option<IntOrStr<'de>>,
    /// criticality
    pub crit: Option<&'de [IntOrStr<'de>]>,
    /// content type
    pub content_type: Option<IntOrStr<'de>>,
    /// key identifier
    pub kid: Option<&'de minicbor::bytes::ByteSlice>,
    /// IV
    pub iv: Option<&'de minicbor::bytes::ByteSlice>,
    /// Partial IV
    pub partial_iv: Option<&'de minicbor::bytes::ByteSlice>,
    rest: HashMap<IntOrStr<'de>, Any<'de>>,
}

pub struct Any<'de>(&'de [u8]);

impl<'de, C> minicbor::Encode<C> for Any<'de> {
    fn encode<W: encode::Write>(
        &self,
        e: &mut encode::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), encode::Error<W::Error>> {
        e.writer_mut()
            .write_all(self.0)
            .map_err(encode::Error::write)
    }
}

impl<'de, C> minicbor::Decode<'de, C> for Any<'de> {
    fn decode(
        d: &mut minicbor::Decoder<'de>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor::decode::Error> {
        let starting_pos = d.position();
        d.skip()?;
        let ending_pos = d.position();
        Ok(Self(&d.input()[starting_pos..ending_pos]))
    }
}

impl<'de> HeaderMap<'de> {
    pub fn len(&self) -> u64 {
        let Self {
            alg,
            crit,
            content_type,
            kid,
            iv,
            partial_iv,
            rest,
        } = self;
        let sum: u64 = [
            alg.is_some(),
            crit.is_some(),
            content_type.is_some(),
            kid.is_some(),
            iv.is_some(),
            partial_iv.is_some(),
        ]
        .into_iter()
        .map(u64::from)
        .sum();
        sum + u64::try_from(rest.len()).expect("usize should fit into u64")
    }
}

impl<'de, C> minicbor::Encode<C> for HeaderMap<'de> {
    fn encode<W: encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), encode::Error<W::Error>> {
        e.map(self.len())?;
        let Self {
            alg,
            crit,
            content_type,
            kid,
            iv,
            partial_iv,
            rest,
        } = self;

        if let Some(alg) = alg {
            e.u8(1)?.encode(alg)?;
        }
        if let Some(crit) = crit {
            e.u8(2)?.encode(crit)?;
        }
        if let Some(content_type) = content_type {
            e.u8(3)?.encode(content_type)?;
        }
        if let Some(kid) = kid {
            e.u8(4)?.encode(kid)?;
        }
        if let Some(iv) = iv {
            e.u8(5)?.encode(iv)?;
        }
        if let Some(partial_iv) = partial_iv {
            e.u8(6)?.encode(partial_iv)?;
        }
        // raw encode each remaining value
        for (key, value) in rest.iter() {
            e.encode(key)?
                .writer_mut()
                .write_all(value.0)
                .map_err(encode::Error::write)?;
        }

        todo!()
    }
}

pub struct Headers<'a> {
    protected: &'a [u8],
    unprotected: HeaderMap<'a>,
}

#[cfg(test)]
mod tests {
    mod int_or_str {
        use crate::tests::diag_to_bytes;
        use crate::types::IntOrStr;

        #[test]
        fn int() -> Result<(), anyhow::Error> {
            let bytes = diag_to_bytes("-7");
            let out: IntOrStr = minicbor::decode(&bytes)?;
            assert_eq!(IntOrStr::Int(-7), out);
            assert_eq!(bytes, minicbor::to_vec(&out)?);
            Ok(())
        }
        #[test]
        fn str() -> Result<(), anyhow::Error> {
            let bytes = diag_to_bytes(r#""test""#);
            let out: IntOrStr = minicbor::decode(&bytes)?;
            assert_eq!(IntOrStr::Str("test"), out);
            assert_eq!(bytes, minicbor::to_vec(&out)?);
            Ok(())
        }
    }
}

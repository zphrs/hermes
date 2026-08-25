use minicbor::{decode, encode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntOrStr<'a> {
    Int(i64),
    Str(&'a str),
}

impl<'a, C> minicbor::Encode<C> for IntOrStr<'a> {
    fn encode<W: encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), encode::Error<W::Error>> {
        match self {
            IntOrStr::Int(i) => e.i64(*i)?,
            IntOrStr::Str(s) => e.str(s)?,
        };
        Ok(())
    }
}

impl<'a, C> minicbor::Decode<'a, C> for IntOrStr<'a> {
    fn decode(d: &mut minicbor::Decoder<'a>, _ctx: &mut C) -> Result<Self, decode::Error> {
        let mut probe1 = d.probe();
        if let Ok(int) = probe1.i64() {
            let probe_pos = probe1.position();
            d.set_position(probe_pos);
            Ok(Self::Int(int))
        } else if let Ok(str) = d.str() {
            Ok(Self::Str(str))
        } else {
            Err(decode::Error::type_mismatch(d.datatype()?).at(d.position()))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::IntOrStr;
    use crate::tests::diag_to_bytes;

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

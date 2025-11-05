use super::InitWith;
use crate::sky_capnp::id::{Builder, Reader};

pub type Id = kademlia::Id<32>;

impl<'a> InitWith<'a, Id> for Builder<'a> {
    fn with(mut self, other: &Id) -> Result<(), capnp::Error> {
        for (i, chunk) in other
            .bytes()
            .to_owned()
            .into_iter()
            .array_chunks::<8>()
            .enumerate()
        {
            match i {
                0 => self.set_key0(u64::from_be_bytes(chunk)),
                1 => self.set_key1(u64::from_be_bytes(chunk)),
                2 => self.set_key2(u64::from_be_bytes(chunk)),
                3 => self.set_key3(u64::from_be_bytes(chunk)),
                _ => unreachable!("Id should have exactly 4 chunks of 8 bytes each"),
            }
        }
        Ok(())
    }
}

impl<'a> From<Reader<'a>> for Id {
    fn from(value: Reader<'a>) -> Self {
        let bytes: [u8; 32] = [
            value.get_key0().to_be_bytes(),
            value.get_key1().to_be_bytes(),
            value.get_key2().to_be_bytes(),
            value.get_key3().to_be_bytes(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .try_into()
        .expect("there to be exactly 32 bytes (8*8)");

        bytes.into()
    }
}

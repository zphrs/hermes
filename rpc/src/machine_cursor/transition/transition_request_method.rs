use std::{convert::Infallible, marker::PhantomData, sync::LazyLock};

use maxlen::MaxLen;
use minicbor::bytes::ByteVec;

use crate::{RpcMessage, method::is_leaf};

pub struct TransitionRequestMethod<M: crate::Method> {
    marker: PhantomData<M>,
}

impl<'a, M: crate::Method> crate::Method for TransitionRequestMethod<M> {
    type Req = M::Req;

    type Res = Res;

    type CanTransition = M::CanTransition;

    type IsLeaf = is_leaf::True;
}

/// Sets the tiebreak boolean flag
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct Res {
    #[n(0)]
    in_tiebreak: bool,
    #[n(1)]
    // sub-optimal; ideally would avoid a copy here on deserialization, possibly
    // with Cow
    res: ByteVec,
}

impl MaxLen for Res {
    fn biggest_instantiation() -> Self {
        Self {
            in_tiebreak: true,
            res: Vec::new().into(),
        }
    }

    fn max_len_init() -> usize {
        // 65k max size of inner serialized type seems reasonable as an upper
        // threshold
        minicbor::len(Self::biggest_instantiation()) + u16::MAX as usize
    }

    fn max_len() -> usize {
        static LEN: LazyLock<usize> = LazyLock::new(Res::max_len_init);
        *LEN
    }
}

impl Res {
    pub fn new<'b, Inner: RpcMessage>(
        res: &'b Inner,
        in_tiebreak: bool,
    ) -> Result<Self, minicbor::encode::Error<Infallible>> {
        let max_len = Self::max_len::<Inner>();
        // 4 bytes for length prefix
        let mut bytes_mut = Vec::with_capacity(max_len + 4);
        let mut writer = minicbor_io::Writer::new(&mut bytes_mut);
        writer.set_max_len(max_len as u32);
        writer.write(&res).expect("write should succeed");
        Ok(Self {
            in_tiebreak,
            res: minicbor::to_vec(res)?.into(),
        })
    }

    pub fn max_len<Inner: RpcMessage>() -> usize {
        Inner::max_len()
            + minicbor::len(Self {
                res: Vec::new().into(),
                in_tiebreak: false,
            })
    }

    pub fn into_parts<Inner: RpcMessage>(self) -> (bool, Inner) {
        (self.in_tiebreak, minicbor::decode(&*self.res).unwrap())
    }

    pub(crate) fn set_in_tiebreak(&mut self, in_tiebreak: bool) {
        self.in_tiebreak = in_tiebreak
    }
}

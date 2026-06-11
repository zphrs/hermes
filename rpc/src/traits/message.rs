use minicbor::{Decode, Encode};

pub trait RpcMessage: Encode<()> + maxlen::MaxLen + for<'a> Decode<'a, ()> {}

impl<T> RpcMessage for T where T: Encode<()> + maxlen::MaxLen + for<'a> Decode<'a, ()> {}

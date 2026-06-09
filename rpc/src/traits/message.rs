use std::fmt::Debug;

use minicbor::{Decode, Encode};

pub trait RpcMessage: Debug + for<'a> Decode<'a, ()> + Encode<()> + maxlen::MaxLen {}

impl<T> RpcMessage for T where T: Debug + for<'a> Decode<'a, ()> + Encode<()> + maxlen::MaxLen {}

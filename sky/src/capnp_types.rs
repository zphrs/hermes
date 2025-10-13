pub mod addr;
mod id;
use std::any::Any;

use capnp::{message::HeapAllocator, traits::FromPointerBuilder};
pub use id::Id;
pub mod ip;
pub mod node;
mod rpc;
pub use rpc::peer_request::*;

use crate::capnp_types::rpc::peer_request;

pub trait InitWith<'a, T>
where
    Self: capnp::traits::FromPointerBuilder<'a>,
{
    fn with(self, other: &T) -> Result<(), capnp::Error>;
}

pub trait FillBuilder<CapnpType: ?Sized> {
    fn fill_builder(&self, builder: CapnpType) -> Result<(), capnp::Error>;
}

impl<'a, CapnpType: InitWith<'a, T>, T> FillBuilder<CapnpType> for T {
    fn fill_builder(&self, builder: CapnpType) -> Result<(), capnp::Error> {
        builder.with(self)
    }
}

pub trait FillMessage<'a> {
    fn fill_message<CapnpType: InitWith<'a, Self> + FromPointerBuilder<'a>>(
        &'a self,
        msg: &'a mut capnp::message::Builder<HeapAllocator>,
    ) where
        Self: Sized;
}

impl<'a, T> FillMessage<'a> for T {
    fn fill_message<CapnpType: InitWith<'a, Self> + FromPointerBuilder<'a>>(
        &self,
        msg: &'a mut capnp::message::Builder<HeapAllocator>,
    ) {
        msg.init_root::<CapnpType>().with(self).unwrap();
    }
}

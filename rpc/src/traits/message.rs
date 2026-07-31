use minicbor::{Decode, Encode};

#[diagnostic::on_unimplemented(
    message = "RpcMessage is not implemented for `{Self}`",
    note = "Needs to implement minicbor::Encode, minicbor::Decode, and maxlen::MaxLen.",
    note = "All three can be implemented by putting #[derive(minicbor::Encode, minicbor::Decode, MaxLen)] above the definition for `{Self}`."
)]
pub trait RpcMessage: Encode<()> + maxlen::MaxLen + for<'a> Decode<'a, ()> {}

impl<T> RpcMessage for T where T: Encode<()> + maxlen::MaxLen + for<'a> Decode<'a, ()> {}

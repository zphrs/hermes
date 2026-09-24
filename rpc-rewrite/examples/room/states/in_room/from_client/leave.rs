use super::super::super::Entrypoint;
use rpc_rewrite::{cursor::state, method};

pub struct Method;

impl method::Method for Method {
    type Req<'buf> = ();
    type Res<'buf> = state::Wrapper<Entrypoint>;

    type Type = method::LeafTransition;
}

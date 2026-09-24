use super::super::Entrypoint;
use rpc_rewrite::{cursor::state, method};

/// Used both as the client's handler for a server kick and as a child of
/// [`super::from_client::Method`] (client closes room).
pub struct Method;

impl method::Method for Method {
    type Req<'buf> = ();
    type Res<'buf> = state::Wrapper<Entrypoint>;

    type Type = method::LeafTransition;
}

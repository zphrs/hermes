use super::super::super::super::max_len_str::MaxLenStr;
use rpc_rewrite::method;

pub struct Method;

impl method::Method for Method {
    type Req<'buf> = MaxLenStr<1024>;
    type Res<'buf> = ();

    type Type = method::LeafLoopback;
}

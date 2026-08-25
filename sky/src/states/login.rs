use rpc::method::{can_transition, is_leaf};

pub mod as_earth;
pub mod as_sky;

pub struct Method;

pub enum ReqAs {
    Sky(as_sky::Req),
    Earth(as_earth::Req),
}

pub enum ResAs {
    Sky(as_sky::Res),
    Earth(as_earth::Res),
}

impl rpc::Method for Method {
    type Req = ReqAs;

    type Res = ResAs;

    type CanTransition = can_transition::True;

    type IsLeaf = is_leaf::False;
}

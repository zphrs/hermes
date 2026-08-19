use rpc::{
    method::{Ancestor, FromDescendant, can_transition, is_leaf},
    state,
};

use crate::states::in_room::{close, from_client};

pub mod post {
    use rpc::method::{can_transition, is_leaf};

    use crate::max_len_str::MaxLenStr;

    pub struct Method;

    impl rpc::Method for Method {
        type Req = MaxLenStr<1024>;

        type Res = ();

        type CanTransition = can_transition::False;

        type IsLeaf = is_leaf::True;
    }
}
pub mod loopback {
    use rpc::method::{Ancestor, FromDescendant, can_transition, is_leaf};

    use super::post;

    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
    pub enum Req {
        /// send a message
        #[n(0)]
        Post(#[n(0)] <post::Method as rpc::Method>::Req),
    }

    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
    pub enum Res {
        #[n(0)]
        Post(#[n(0)] <post::Method as rpc::Method>::Res),
    }

    pub struct Method;

    impl rpc::Method for Method {
        type Req = Req;

        type Res = Res;

        type CanTransition = can_transition::False;

        type IsLeaf = is_leaf::False;
    }

    impl Ancestor<post::Method> for Method {}

    impl FromDescendant<post::Method> for Method {
        fn from_descendant_req(
            request: <post::Method as rpc::Method>::Req,
        ) -> <Self as rpc::Method>::Req {
            Req::Post(request)
        }

        fn from_descendant_res(
            result: <post::Method as rpc::Method>::Res,
        ) -> <Self as rpc::Method>::Res {
            Res::Post(result)
        }

        fn try_into_descendant_req(
            request: Self::Req,
        ) -> Result<<post::Method as rpc::Method>::Req, Self::Req> {
            #[allow(unreachable_patterns)]
            match request {
                Req::Post(child) => Ok(child),
                other => Err(other),
            }
        }
    }
}
pub mod leave {
    use rpc::{
        method::{can_transition, is_leaf},
        state,
    };

    use crate::states::Entrypoint;

    pub struct Method;
    impl rpc::Method for Method {
        type Req = ();

        type Res = state::Wrapper<Entrypoint>;

        type CanTransition = can_transition::True;

        type IsLeaf = is_leaf::True;
    }
}
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
pub enum Req {
    /// set which events client is subscribed to
    #[n(0)]
    Loopback(#[n(0)] <loopback::Method as rpc::Method>::Req),
    /// close room
    #[n(1)]
    Close(#[n(0)] <close::Method as rpc::Method>::Req),
    // leave room
    #[n(2)]
    Leave,
}

pub enum Res {
    Loopback(<loopback::Method as rpc::Method>::Res),
    Close(state::Wrapper<crate::states::Entrypoint>),
    Leave(state::Wrapper<crate::states::Entrypoint>),
}

impl state::Has<crate::states::Entrypoint> for Res {
    fn try_extract_wrapper(self) -> Result<state::Wrapper<crate::states::Entrypoint>, Self> {
        match self {
            Res::Loopback(_) => Err(self),
            Res::Close(wrapper) | Res::Leave(wrapper) => Ok(wrapper),
        }
    }
}

pub struct Method;

impl rpc::Method for Method {
    type Req = Req;

    type Res = Res;

    type CanTransition = can_transition::True;

    type IsLeaf = is_leaf::False;
}

impl Ancestor<post::Method> for Method {}
impl Ancestor<loopback::Method> for Method {}
impl Ancestor<close::Method> for Method {}
impl Ancestor<leave::Method> for Method {}

impl FromDescendant<loopback::Method> for Method {
    fn from_descendant_req(
        request: <loopback::Method as rpc::Method>::Req,
    ) -> <Self as rpc::Method>::Req {
        Req::Loopback(request)
    }

    fn from_descendant_res(
        result: <loopback::Method as rpc::Method>::Res,
    ) -> <Self as rpc::Method>::Res {
        Res::Loopback(result)
    }

    fn try_into_descendant_req(
        request: Self::Req,
    ) -> Result<<loopback::Method as rpc::Method>::Req, Self::Req> {
        match request {
            Req::Loopback(subscribe_flags) => Ok(subscribe_flags),
            other => Err(other),
        }
    }
}

impl FromDescendant<leave::Method> for Method {
    fn from_descendant_req(
        _request: <leave::Method as rpc::Method>::Req,
    ) -> <Self as rpc::Method>::Req {
        Req::Leave
    }

    fn from_descendant_res(
        result: <leave::Method as rpc::Method>::Res,
    ) -> <Self as rpc::Method>::Res {
        Res::Leave(result)
    }

    fn try_into_descendant_req(
        request: Self::Req,
    ) -> Result<<leave::Method as rpc::Method>::Req, Self::Req> {
        match request {
            Req::Leave => Ok(()),
            other => Err(other),
        }
    }
}

impl FromDescendant<close::Method> for Method {
    fn from_descendant_req(
        request: <close::Method as rpc::Method>::Req,
    ) -> <Self as rpc::Method>::Req {
        Req::Close(request)
    }

    fn from_descendant_res(
        result: <close::Method as rpc::Method>::Res,
    ) -> <Self as rpc::Method>::Res {
        Res::Close(result)
    }

    fn try_into_descendant_req(
        request: Self::Req,
    ) -> Result<<close::Method as rpc::Method>::Req, Self::Req> {
        match request {
            Req::Close(child) => Ok(child),
            other => Err(other),
        }
    }
}

impl FromDescendant<from_client::post::Method> for Method {
    fn from_descendant_req(
        request: <from_client::post::Method as rpc::Method>::Req,
    ) -> <Self as rpc::Method>::Req {
        Req::Loopback(loopback::Req::Post(request))
    }

    fn from_descendant_res(
        result: <from_client::post::Method as rpc::Method>::Res,
    ) -> <Self as rpc::Method>::Res {
        Res::Loopback(loopback::Res::Post(result))
    }

    fn try_into_descendant_req(
        request: Self::Req,
    ) -> Result<<from_client::post::Method as rpc::Method>::Req, Self::Req> {
        match request {
            Req::Loopback(loopback::Req::Post(child)) => Ok(child),
            other => Err(other),
        }
    }
}

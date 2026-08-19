use rpc::state;

use crate::states::{Entrypoint, logged_in::LoggedIn};

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
#[cbor(flat)]
pub enum Error {
    #[n(0)]
    UserNotFound(#[n(0)] state::Wrapper<Entrypoint>),
    #[n(1)]
    PasswordIncorrect(#[n(0)] state::Wrapper<Entrypoint>),
}

impl Error {
    pub fn user_not_found<RootMethod>(
        replier: &impl rpc::transport::ReplyHelper<RootMethod, super::Method>,
    ) -> Self {
        Self::UserNotFound(replier.new_wrapper())
    }

    pub fn password_incorrect<RootMethod>(
        replier: &impl rpc::transport::ReplyHelper<RootMethod, super::Method>,
    ) -> Self {
        Self::PasswordIncorrect(replier.new_wrapper())
    }

    pub fn extract_wrapper(self) -> rpc::state::Wrapper<Entrypoint> {
        match self {
            Error::UserNotFound(wrapper) | Error::PasswordIncorrect(wrapper) => wrapper,
        }
    }
}

impl rpc::state::Has<Entrypoint> for Error {
    fn try_extract_wrapper(self) -> Result<rpc::state::Wrapper<Entrypoint>, Self> {
        Ok(self.extract_wrapper())
    }
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
#[cbor(transparent)]
pub struct Res(Result<state::Wrapper<LoggedIn>, Error>);
impl Res {
    pub(crate) fn into_inner(self) -> Result<state::Wrapper<LoggedIn>, Error> {
        self.0
    }
}

impl From<Error> for Res {
    fn from(value: Error) -> Self {
        Self(Err(value))
    }
}

impl From<state::Wrapper<LoggedIn>> for Res {
    fn from(value: state::Wrapper<LoggedIn>) -> Self {
        Self(Ok(value))
    }
}

impl state::Has<LoggedIn> for Res {
    fn try_extract_wrapper(self) -> Result<rpc::state::Wrapper<LoggedIn>, Self> {
        match self.0 {
            Ok(wrapper) => Ok(wrapper),
            Err(_) => Err(self),
        }
    }
}

impl state::Has<Entrypoint> for Res {
    fn try_extract_wrapper(self) -> Result<rpc::state::Wrapper<Entrypoint>, Self> {
        match self.0 {
            Ok(_) => Err(self),
            Err(wrapper) => Ok(wrapper.try_extract_wrapper().unwrap()),
        }
    }
}

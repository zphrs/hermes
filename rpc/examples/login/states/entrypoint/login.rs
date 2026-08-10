mod req;
mod res;
pub use req::Req;
pub use res::{Error, Res};

use std::convert::Infallible;

use rpc::method::{can_transition, is_leaf};

pub struct Method;

impl rpc::Method for Method {
    /// password
    type Req = Req;

    type Res = Res;

    type CanTransition = can_transition::True;

    type IsLeaf = is_leaf::True;
}

// doesn't care about root method implementation
impl<RootMethod: rpc::method::Ancestor<Method>> rpc::Handler<RootMethod> for Method {
    // no reason not to reply & propagate error up to the processor Result.
    type Error = Infallible;

    fn handle<Replier: rpc::ReplyHelper<RootMethod, Self>>(
        &mut self,
        replier: Replier,
        Req { username, password }: rpc::ReqOf<Self>,
    ) -> impl Future<Output = rpc::traits::HandlerResult<RootMethod, Self, Replier, Self::Error>>
    {
        match (username.as_str(), password.as_str()) {
            ("admin", "password") => {
                let wrapper = replier.new_wrapper();
                replier.reply(wrapper.into())
            }
            ("admin", _) => {
                let password_incorrect = res::Error::password_incorrect(&replier);
                replier.reply(password_incorrect.into())
            }
            _ => {
                let user_not_found = res::Error::user_not_found(&replier);
                replier.reply(user_not_found.into())
            }
        }
    }
}

use crate::ReqOf;

#[derive(Debug, thiserror::Error)]
pub enum HandleError<Replier, Handler> {
    #[error("Replier: {0}")]
    Replier(Replier),
    #[error("Handler: {0}")]
    Handler(#[from] Handler),
}

pub type HandlerResult<RootMethod, Method, Replier, Error> = Result<
    <Replier as crate::ReplyHelper<RootMethod, Method>>::Receipt<Method>,
    HandleError<<Replier as crate::ReplyHelper<RootMethod, Method>>::Error, Error>,
>;

pub trait Handler<RM, Method: crate::Method = Self> {
    /// used to abort a reply midway through handling a request
    type Error;
    fn handle<Replier: crate::ReplyHelper<RM, Method>>(
        &mut self,
        replier: Replier,
        value: ReqOf<Method>,
    ) -> impl Future<Output = HandlerResult<RM, Method, Replier, Self::Error>>;
}

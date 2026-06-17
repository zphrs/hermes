use crate::transport::ReplyHelper;

#[derive(Debug, thiserror::Error)]
pub enum HandleError<Replier, Handler> {
    #[error("Replier: {0}")]
    Replier(Replier),
    #[error("Handler: {0}")]
    Handler(#[from] Handler),
}

pub trait Handler<Method: crate::Method = Self> {
    /// used to abort a reply midway through handling a request
    type Error;
    fn handle<Replier: ReplyHelper<Method>>(
        &mut self,
        replier: Replier,
        value: Method::Req,
    ) -> impl Future<
        Output = Result<
            <Replier as ReplyHelper<Method>>::Receipt<Method>,
            HandleError<<Replier as ReplyHelper<Method>>::Error, <Self as Handler<Method>>::Error>,
        >,
    >;
}

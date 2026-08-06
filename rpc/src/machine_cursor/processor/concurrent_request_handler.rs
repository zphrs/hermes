use crate::{
    method::{FromDescendant, is_leaf},
    traits::{
        self, HandleError,
        method::{Loopback, can_transition},
    },
    transport::ReplyHelper,
};
use std::marker::PhantomData;

#[derive(Clone)]
pub struct ConcurrentRequestHandler<
    ParentMethod: crate::Method,
    LoopbackMethod: Loopback,
    LoopbackHandler: traits::Handler<ParentMethod, LoopbackMethod>,
> {
    handler: LoopbackHandler,
    _marker: PhantomData<(ParentMethod, LoopbackMethod)>,
}

impl<
    ParentMethod: FromDescendant<LoopbackMethod, IsLeaf = is_leaf::False>,
    LoopbackMethod: Loopback,
    LoopbackHandler: traits::Handler<ParentMethod, LoopbackMethod>,
> crate::Handler<ParentMethod, ParentMethod>
    for ConcurrentRequestHandler<ParentMethod, LoopbackMethod, LoopbackHandler>
{
    type Error = ConcurrentRequestHandlerError<LoopbackHandler::Error, ParentMethod::Req>;

    async fn handle<Replier: ReplyHelper<ParentMethod, ParentMethod>>(
        &mut self,
        replier: Replier,
        value: <ParentMethod as crate::Method>::Req,
    ) -> Result<Replier::Receipt<ParentMethod>, HandleError<Replier::Error, Self::Error>> {
        let parallel_req = match ParentMethod::try_into_descendant_req(value) {
            Ok(v) => v,
            Err(root) => {
                return Err(ConcurrentRequestHandlerError::FailedConversion(root))?;
            }
        };

        let res = replier
            .reply_with::<LoopbackMethod, LoopbackHandler>(&mut self.handler, parallel_req, |v| {
                ParentMethod::from_descendant_res(v)
            })
            .await;
        let res = res.map_err(|e| match e {
            HandleError::Replier(r) => HandleError::Replier(r),
            HandleError::Handler(app) => {
                HandleError::Handler(ConcurrentRequestHandlerError::ParallelHandler(app))
            }
        })?;

        Ok(res)
    }
}

impl<ParentMethod, LoopbackMethod, LoopbackHandler> crate::Method
    for ConcurrentRequestHandler<ParentMethod, LoopbackMethod, LoopbackHandler>
where
    ParentMethod: crate::Method,
    LoopbackMethod: Loopback,
    LoopbackHandler: traits::Handler<ParentMethod, LoopbackMethod>,
{
    type Req = ParentMethod::Req;

    type Res = LoopbackMethod::Res;

    type CanTransition = can_transition::False;

    type IsLeaf = is_leaf::False;
}

impl<
    ParentMethod: crate::Method,
    LoopbackMethod: Loopback,
    LoopbackHandler: traits::Handler<ParentMethod, LoopbackMethod>,
> ConcurrentRequestHandler<ParentMethod, LoopbackMethod, LoopbackHandler>
{
    pub fn new(handler: LoopbackHandler) -> Self {
        Self {
            handler,
            _marker: PhantomData,
        }
    }
}

pub enum ConcurrentRequestHandlerError<ParallelError, Root> {
    ParallelHandler(ParallelError),
    FailedConversion(Root),
}

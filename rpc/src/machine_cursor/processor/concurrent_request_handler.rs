use crate::method::{Ancestor, is_leaf};
use crate::traits::HandleError;
use crate::traits::method::can_transition;
use crate::traits::{self, method::Loopback};
use crate::transport::ReplyHelper;
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
    ParentMethod: crate::Method,
    LoopbackMethod: Loopback,
    LoopbackHandler: traits::Handler<ParentMethod, LoopbackMethod>,
> Ancestor<ConcurrentRequestHandler<ParentMethod, LoopbackMethod, LoopbackHandler>>
    for ParentMethod
{
}

impl<
    ParentMethod: crate::Method + Ancestor<LoopbackMethod>,
    LoopbackMethod: Loopback,
    LoopbackHandler: traits::Handler<ParentMethod, LoopbackMethod>,
> crate::Handler<ParentMethod>
    for ConcurrentRequestHandler<ParentMethod, LoopbackMethod, LoopbackHandler>
where
    LoopbackMethod::Req: TryFrom<ParentMethod::Req, Error = ParentMethod::Req>,
{
    type Error = ConcurrentRequestHandlerError<LoopbackHandler::Error, ParentMethod::Req>;

    async fn handle<Replier: ReplyHelper<Self, ParentMethod>>(
        &mut self,
        replier: Replier,
        value: <Self as crate::Method>::Req,
    ) -> Result<
        <Replier as ReplyHelper<Self, ParentMethod>>::Receipt<Self>,
        HandleError<
            <Replier as ReplyHelper<Self, ParentMethod>>::Error,
            <Self as crate::Handler<ParentMethod, Self>>::Error,
        >,
    > {
        let parallel_req = match LoopbackMethod::Req::try_from(value) {
            Ok(v) => v,
            Err(root) => {
                return Err(ConcurrentRequestHandlerError::FailedConversion(root))?;
            }
        };

        let res = replier
            .reply_with::<LoopbackMethod, LoopbackHandler>(&mut self.handler, parallel_req, |v| v)
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

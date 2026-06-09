use crate::MethodWrapper;

pub struct Parts<WrapperOrRes, Caller: crate::Caller>(pub WrapperOrRes, pub Caller);

impl<WrapperOrRes, Caller: crate::Caller> Parts<WrapperOrRes, Caller> {
    pub fn set_method<Method: crate::Method>(
        self,
        wrapper: MethodWrapper<Method>,
    ) -> Parts<MethodWrapper<Method>, Caller> {
        Parts(wrapper, self.1)
    }

    pub fn extract_res(self) -> (WrapperOrRes, Parts<(), Caller>) {
        let Self(out, caller) = self;

        (out, Parts((), caller))
    }

    pub fn map_res<Method: crate::Method>(
        self,
        mapper: impl FnOnce(WrapperOrRes) -> MethodWrapper<Method>,
    ) -> Parts<MethodWrapper<Method>, Caller> {
        Parts(mapper(self.0), self.1)
    }
}

pub struct Wrapper<Handler: crate::Method, Caller: crate::Caller> {
    wrapper: MethodWrapper<Handler>,
    conn: Caller,
}

impl<Method: crate::Method + Send, Caller: crate::Caller> Wrapper<Method, Caller>
where
    Method::Req: crate::RpcMessage,
{
    pub fn new(conn: Caller) -> Wrapper<Method, Caller> {
        Self {
            wrapper: MethodWrapper::new(),
            conn,
        }
    }

    pub fn conn(&self) -> &Caller {
        &self.conn
    }

    /// intentionally private to ensure that the conn is not accessed except
    /// when necessary to transition between states after a query request
    fn into_parts(self) -> Parts<MethodWrapper<Method>, Caller> {
        Parts(self.wrapper, self.conn)
    }

    pub fn from_parts(
        Parts(wrapper, conn): Parts<MethodWrapper<Method>, Caller>,
    ) -> Wrapper<Method, Caller> {
        Self { wrapper, conn }
    }

    pub async fn query<M: crate::Method>(
        self,
        req: M::Req,
    ) -> Result<Parts<M::Res, Caller>, crate::transport::CallerError<Caller::Error>>
    where
        Method::Req: crate::RpcMessage + From<M::Req> + Send,
        M::Res: crate::RpcMessage,
    {
        let Parts(wrapper, conn) = self.into_parts();
        Ok(Parts(wrapper.query::<M, _>(req, &conn).await?, conn))
    }

    pub async fn query_loopback_child<M: crate::Method, ParallelHandler>(
        &self,
        req: M::Req,
    ) -> Result<M::Res, crate::transport::CallerError<Caller::Error>>
    where
        ParallelHandler: crate::Call,
        ParallelHandler::Req: From<M::Req>,
        Method::Req: crate::RpcMessage + From<ParallelHandler::Req> + Send,
        Method::Req: From<M::Req>,
        MethodWrapper<Method>: From<ParallelHandler::Res>,
        M::Res: crate::RpcMessage,
    {
        self.wrapper
            .query_loopback_child::<M, _, ParallelHandler>(req, &self.conn)
            .await
    }

    pub async fn query_loopback<M: crate::Method>(
        &self,
        req: M::Req,
    ) -> Result<M::Res, crate::transport::CallerError<Caller::Error>>
    where
        Method: crate::Call,
        Method::Req: From<M::Req>,
        Method::Req: crate::RpcMessage + From<Method::Req> + Send,
        Method::Req: From<M::Req>,
        MethodWrapper<Method>: From<Method::Res>,
        M::Res: crate::RpcMessage,
    {
        self.wrapper.query_loopback::<M, _>(req, &self.conn).await
    }
}

impl<Method: crate::Method + Send, Caller: crate::Caller> From<Parts<MethodWrapper<Method>, Caller>>
    for Wrapper<Method, Caller>
where
    Method::Req: crate::RpcMessage,
{
    fn from(val: Parts<MethodWrapper<Method>, Caller>) -> Self {
        Self::from_parts(val)
    }
}

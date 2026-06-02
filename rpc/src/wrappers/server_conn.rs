use crate::MethodWrapper;

pub struct Parts<WrapperOrRes, Caller: crate::transport::Client, Handler: crate::Call>(
    pub WrapperOrRes,
    pub Caller,
    pub Handler,
);

impl<WrapperOrRes, Client: crate::transport::Client, Handler: crate::Call>
    Parts<WrapperOrRes, Client, Handler>
{
    pub fn method_change<NewHandler: crate::Call>(
        self,
        wrapper: MethodWrapper<NewHandler>,
        handler: NewHandler,
    ) -> Parts<MethodWrapper<NewHandler>, Client, NewHandler> {
        Parts(wrapper, self.1, handler)
    }

    pub fn extract_res(self) -> (WrapperOrRes, Parts<(), Client, Handler>) {
        let Self(out, caller, handler) = self;

        (out, Parts((), caller, handler))
    }

    pub fn method_restore(
        self,
        wrapper: MethodWrapper<Handler>,
    ) -> Parts<MethodWrapper<Handler>, Client, Handler> {
        Parts(wrapper, self.1, self.2)
    }
}

pub struct Wrapper<Handler: crate::Call, Client: crate::transport::Client> {
    wrapper: MethodWrapper<Handler>,
    handler: Handler,
    conn: Client,
}

impl<Handler: crate::Call + Send, Client: crate::transport::Client> Wrapper<Handler, Client>
where
    Handler::Req: crate::RpcMessage,
{
    pub fn new(handler: Handler, conn: Client) -> Wrapper<Handler, Client> {
        Self {
            handler,
            wrapper: MethodWrapper::new(),
            conn,
        }
    }

    fn into_parts(self) -> Parts<MethodWrapper<Handler>, Client, Handler> {
        Parts(self.wrapper, self.conn, self.handler)
    }
    pub fn from_parts(
        Parts(wrapper, conn, handler): Parts<MethodWrapper<Handler>, Client, Handler>,
    ) -> Self {
        Self {
            handler,
            wrapper,
            conn,
        }
    }

    pub async fn handle_state_transition(
        self,
    ) -> Result<
        Parts<Handler::Res, Client, Handler>,
        crate::ClientError<Client::Error, Handler::Error>,
    >
    where
        Handler: crate::Call + Send,
        Handler::Req: crate::RpcMessage,
        Handler::Error: std::marker::Send,
    {
        let Parts(wrapper, conn, mut handler) = self.into_parts();

        let stream = conn
            .accept_stream()
            .await
            .map_err(crate::ClientError::Transport)?;

        let res = wrapper
            .handle_state_transition_request(&conn, stream, &mut handler)
            .await?;

        Ok(Parts(res, conn, handler))
    }

    pub fn handle_loopback_partial<'a, ParallelHandler>(
        self,
        parallel_handler: ParallelHandler,
    ) -> impl Future<
        Output = Result<
            Parts<Handler::Res, Client, Handler>,
            crate::ClientError<Client::Error, Handler::Error>,
        >,
    > + Send
    where
        Handler::Req: crate::RpcMessage
            + Send
            + std::marker::Sync
            + From<<ParallelHandler::Req as TryFrom<Handler::Req>>::Error>,
        Handler: crate::Call,
        ParallelHandler: crate::Call + std::marker::Send + Clone,
        ParallelHandler::Req: TryFrom<Handler::Req> + crate::RpcMessage + std::marker::Send,
        ParallelHandler::Error: std::marker::Send,
        ParallelHandler::Res: Sync,
        Handler::Req: From<<ParallelHandler::Req as TryFrom<Handler::Req>>::Error>,
        Handler::Error: From<ParallelHandler::Error>,
    {
        async {
            let Parts(wrapper, conn, mut handler) = self.into_parts();

            let res = wrapper
                .handle_concurrent_requests(&conn, parallel_handler, &mut handler)
                .await?;

            Ok(Parts(res, conn, handler))
        }
    }

    pub async fn handle_loopback(
        self,
    ) -> Result<(), crate::ClientError<Client::Error, Handler::Error>>
    where
        Handler::Req: crate::RpcMessage + Send + std::marker::Sync,
        Handler: crate::Call + std::marker::Send + Clone,
        Handler::Req: TryFrom<Handler::Req> + crate::RpcMessage + std::marker::Send,
        Handler::Error: std::marker::Send,
        Handler::Res: Sync,
        Handler::Error: From<Handler::Error> + From<<Handler::Req as TryFrom<Handler::Req>>::Error>,
    {
        let Parts(wrapper, conn, handler) = self.into_parts();

        let _res = wrapper
            .handle_only_concurrent_requests(&conn, handler)
            .await?;

        Ok(())
    }
}

impl<Handler: crate::Call + Send, Client: crate::transport::Client>
    From<Parts<MethodWrapper<Handler>, Client, Handler>> for Wrapper<Handler, Client>
where
    Handler::Req: crate::RpcMessage,
{
    fn from(value: Parts<MethodWrapper<Handler>, Client, Handler>) -> Self {
        Self::from_parts(value)
    }
}

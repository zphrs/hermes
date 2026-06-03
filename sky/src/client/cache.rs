use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use rpc::{MethodWrapper, Transport as _, client_conn::Wrapper};
use shared_schema::SkyNode;
use tokio::sync::Mutex;

use crate::{
    api::{self},
    quinn_transport::{self, Transport},
};

pub struct Cache<Method: rpc::Method, LoginMethod: rpc::Method> {
    tp: Transport,
    cache: HashMap<SkyNode, Mutex<Option<Arc<Wrapper<Method, crate::quinn_transport::Caller>>>>>,
    login_request: LoginMethod::Req,
    _marker: PhantomData<LoginMethod>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("connection couldn't be established: {0}")]
    Caller(#[from] rpc::CallerError<quinn_transport::Error>),
    #[error("login's result couldn't be converted to the primary method")]
    LoginFailed,
}

impl<Method: rpc::Method + Send, LoginMethod: rpc::Method> Cache<Method, LoginMethod>
where
    Method::Req: rpc::RpcMessage,
    api::entrypoint::Request: From<LoginMethod::Req>,
    LoginMethod::Req: Clone,
{
    pub fn new(tp: Transport, login_request: LoginMethod::Req) -> Self {
        Cache {
            tp,
            cache: HashMap::default(),
            login_request,
            _marker: PhantomData,
        }
    }

    pub async fn connect(
        &mut self,
        node: &SkyNode,
        handle_req: impl FnOnce(LoginMethod::Res) -> Option<MethodWrapper<Method>>,
    ) -> Result<Arc<Wrapper<Method, crate::quinn_transport::Caller>>, ConnectError> {
        let mut entry_lock = self.cache.entry(node.clone()).or_default().lock().await;
        if let Some(cached) = &*entry_lock {
            if cached.conn().inner().close_reason().is_none() {
                return Ok(cached.clone());
            }
            *entry_lock = None;
        }

        let conn = self
            .tp
            .connect(node)
            .await
            .map_err(rpc::CallerError::Transport)?;

        let wrapper: Wrapper<api::entrypoint::Method, _> = Wrapper::new(conn);

        let (res, parts) = wrapper
            .query::<LoginMethod>(self.login_request.clone())
            .await?
            .extract_res();

        let method = handle_req(res).ok_or(ConnectError::LoginFailed)?;

        let wrapper = Arc::new(Wrapper::from(parts.set_method(method)));

        *entry_lock = Some(wrapper.clone());

        Ok(wrapper)
    }
}

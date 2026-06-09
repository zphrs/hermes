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
pub enum ConnectError<'a> {
    #[error("connection couldn't be established: {0}")]
    Caller(#[from] rpc::CallerError<quinn_transport::Error>),
    #[error("login's result couldn't be converted to the primary method")]
    LoginFailed,
    #[error("no entry exists for {0:?}")]
    /// Call [`Cache::insert_node_entry()`] before trying to connect again
    MissingNodeEntry(&'a SkyNode),
}

impl<Method: rpc::Method + Send, LoginMethod: rpc::Method> Cache<Method, LoginMethod>
where
    Method::Req: rpc::RpcMessage,
    LoginMethod::Res: rpc::RpcMessage,
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
    /// if this returns none, one should call insert_node_entry before trying
    /// to connect again.
    pub async fn try_connect<'a>(
        &self,
        node: &'a SkyNode,
        handle_req: &mut impl FnMut(LoginMethod::Res) -> Option<MethodWrapper<Method>>,
    ) -> Result<Arc<Wrapper<Method, crate::quinn_transport::Caller>>, ConnectError<'a>> {
        let mut entry_lock = self
            .cache
            .get(node)
            .ok_or_else(|| ConnectError::MissingNodeEntry(node))?
            .lock()
            .await;
        if let Some(value) = entry_lock.as_ref() {
            if value.conn().inner().close_reason().is_none() {
                return Ok(value.clone());
            }
            *entry_lock = None;
        };

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

    pub fn insert_node_entry(&mut self, node: &SkyNode) {
        self.cache.entry(node.clone()).or_default();
    }

    pub async fn connect<'a>(
        &mut self,
        node: &'a SkyNode,
        mut handle_req: impl FnMut(LoginMethod::Res) -> Option<MethodWrapper<Method>>,
    ) -> Result<Arc<Wrapper<Method, crate::quinn_transport::Caller>>, ConnectError<'a>> {
        loop {
            match self.try_connect(node, &mut handle_req).await {
                Err(ConnectError::MissingNodeEntry(_)) => self.insert_node_entry(node),
                res => return res,
            }
        }
    }
}

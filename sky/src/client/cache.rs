use rpc::{
    machine_cursor::{self, Processor},
    state::role,
};
use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use rpc::{
    MachineCursor, Transport as _, state,
    state::ServerReq,
    traits::method::{
        CanTransition,
        not_applicable::{self, NotApplicable},
    },
};
use shared_schema::SkyNode;
use tokio::sync::Mutex;

use crate::{
    api::{self},
    quinn_transport::{self, Transport},
};

pub type Sender<State> = Arc<
    machine_cursor::Requester<
        role::Client,
        <State as rpc::State>::ServerMethod,
        quinn_transport::Connection,
    >,
>;

pub type Parts<State> = (
    Processor<
        State,
        role::Client,
        NotApplicable,
        quinn_transport::Connection,
        not_applicable::Handler,
    >,
    Sender<State>,
);

pub struct Cache<State: rpc::State, LoginState: rpc::State, LoginMethod: rpc::Method> {
    tp: Transport,
    cache: HashMap<SkyNode, Mutex<Option<Parts<State>>>>,
    login_request: LoginMethod::Req,
    _marker: PhantomData<LoginState>,
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

impl<
    State: rpc::State<ClientMethod = NotApplicable> + Send,
    LoginState: rpc::State<ClientMethod = NotApplicable>,
    LoginMethod: rpc::Method,
> Cache<State, LoginState, LoginMethod>
where
    ServerReq<State>: rpc::RpcMessage,
    ServerReq<LoginState>: rpc::RpcMessage,
    LoginMethod::Req: rpc::RpcMessage + Clone,
    LoginMethod::Res: rpc::RpcMessage + Unpin,
    api::entrypoint::Request: From<<LoginState::ServerMethod as rpc::Method>::Req>,
    LoginMethod: CanTransition,
    ServerReq<LoginState>: From<LoginMethod::Req>,
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
        handle_req: &mut impl FnMut(LoginMethod::Res) -> Option<state::Wrapper<State>>,
    ) -> Result<Sender<State>, ConnectError<'a>> {
        let mut entry_lock = self
            .cache
            .get(node)
            .ok_or_else(|| ConnectError::MissingNodeEntry(node))?
            .lock()
            .await;
        if let Some(value) = entry_lock.as_ref() {
            if value.0.client().inner().close_reason().is_none() {
                return Ok(value.1.clone());
            }
            *entry_lock = None;
        };

        let conn = self
            .tp
            .connect(node)
            .await
            .map_err(rpc::CallerError::Transport)?;

        let cursor = MachineCursor::<LoginState, _, _>::new(conn, rpc::state::role::Client);

        let (handler, querier) = cursor.into_parts(not_applicable::Handler);

        let (res, transition_receipt) = querier
            .request_transition::<LoginMethod>(self.login_request.clone())
            .await?
            .extract_result();

        let method = handle_req(res).ok_or(ConnectError::LoginFailed)?;

        let cursor = MachineCursor::<_, _, state::role::Client>::from_transition_receipt(
            transition_receipt,
            method,
            handler,
        )
        .await?;
        let parts = cursor.into_parts(not_applicable::Handler);

        let wrapper = (parts.0, Arc::new(parts.1));
        let out = wrapper.1.clone();
        *entry_lock = Some(wrapper);

        Ok(out)
    }

    pub fn insert_node_entry(&mut self, node: &SkyNode) {
        self.cache.entry(node.clone()).or_default();
    }

    pub async fn connect<'a>(
        &mut self,
        node: &'a SkyNode,
        mut handle_req: impl FnMut(LoginMethod::Res) -> Option<state::Wrapper<State>>,
    ) -> Result<Sender<State>, ConnectError<'a>> {
        loop {
            match self.try_connect(node, &mut handle_req).await {
                Err(ConnectError::MissingNodeEntry(_)) => self.insert_node_entry(node),
                res => return res,
            }
        }
    }
}

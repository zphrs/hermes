use std::{convert::Infallible, time::Duration};

use rpc::{ClientError, Transport as _};
use shared_schema::SkyNode;
use tokio::task::{JoinHandle, JoinSet};
use tracing::{Instrument as _, info, info_span, instrument::WithSubscriber as _, trace};

use crate::{api::find_nodes::KadRpcManager, entrypoint::as_sky, quinn_transport};
use rpc::server_conn::Wrapper;

#[derive(Clone)]
pub struct SkyServer {
    manager: KadRpcManager,
    transport: quinn_transport::Transport,
}

impl SkyServer {
    pub async fn new() -> Result<Self, ClientError<quinn_transport::Error, Infallible>> {
        let tp = quinn_transport::Transport::self_signed_server()
            .await
            .map_err(ClientError::Transport)?;
        let pub_addr = tp.inner().local_addr().unwrap();
        let manager = KadRpcManager::new(tp.clone(), pub_addr.ip().into());

        Ok(Self {
            manager,
            transport: tp,
        })
    }

    pub fn into_parts(self) -> (KadRpcManager, quinn_transport::Transport) {
        (self.manager, self.transport)
    }
    pub async fn add_nodes(&self, nodes: Vec<SkyNode>) {
        self.manager.inner().add_nodes(nodes).await;
    }
    pub async fn bootstrap(&self) {
        self.manager.inner().join_network().await;
    }

    pub fn run_refresh_loop(&self) {
        let handler = self.clone().into_parts().0;
        let local_node = handler.inner().local_node();
        let span = info_span!("refresh_loop", me = ?local_node);
        tokio::task::spawn(
            async move {
                tokio::time::sleep(Duration::from_secs(rand::random_range((5 * 60)..(10 * 60))))
                    .await;
                // tokio::time::sleep(Duration::from_secs(8 * 60)).await;
                loop {
                    info!("refreshing buckets!");
                    handler
                        .inner()
                        .refresh_stale_buckets(&Duration::from_secs(60 * 65))
                        .await;
                    info!("finished refreshing buckets");
                    tokio::time::sleep(Duration::from_secs(60 * 60)).await;
                }
            }
            .with_current_subscriber()
            .instrument(span),
        );
    }

    pub fn run(&self) -> JoinHandle<Result<(), ClientError<quinn_transport::Error, Infallible>>> {
        self.run_refresh_loop();

        let (manager, tp) = self.clone().into_parts();

        let jh = tokio::task::spawn(
            async move {
                let mut js: JoinSet<Result<(), ClientError<quinn_transport::Error, Infallible>>> =
                    JoinSet::new();
                loop {
                    trace!("awaiting client");
                    let incoming_client: quinn_transport::Incoming =
                        tp.accept().await.map_err(ClientError::Transport)?;
                    let manager = manager.clone();
                    let span = info_span!("handling request",
                        self_addr = %incoming_client.inner().local_ip().unwrap(),
                        client = %incoming_client.inner().remote_address()
                    );
                    js.spawn(
                        async move {
                            trace!("accepting {}", incoming_client.inner().remote_address());
                            let conn = match rpc::transport::Incoming::accept(incoming_client).await
                            {
                                Ok(v) => v,
                                Err(e) => {
                                    info!("timing out...");
                                    Err(e).map_err(ClientError::Transport)?
                                }
                            };
                            let handler = crate::api::entrypoint::Method::new(
                                conn.inner().remote_address().ip(),
                            );
                            let mut login = Wrapper::new(handler, conn);
                            let ((actions, parts), sky_node) = loop {
                                let (res, parts) =
                                    login.handle_state_transition().await?.extract_res();
                                use crate::entrypoint::Response::*;
                                match res {
                                    Sky(as_sky::Response::Ok(actions), sky_node) => {
                                        // continue on
                                        break ((actions, parts), sky_node);
                                    }
                                    Sky(as_sky::Response::Invalid(loopback), _sky_node) => {
                                        // restore login from the parts
                                        login = Wrapper::from(parts.method_restore(loopback));
                                    }
                                }
                            };
                            let handler = crate::api::sky_root::Method::new(&manager, sky_node);
                            let logged_in = Wrapper::from(parts.method_change(actions, handler));
                            logged_in.handle_loopback().await?;

                            Ok(())
                        }
                        .instrument(span),
                    );
                    while let Some(result) = js.try_join_next() {
                        if let Err(e) = result.unwrap() {
                            match e {
                                ClientError::Transport(quinn_transport::Error::Connection(
                                    quinn::ConnectionError::TimedOut,
                                )) => {
                                    tracing::warn!("timed out!");
                                }
                                e => {
                                    tracing::error!("error while handling client: {e}: {e:?}");
                                }
                            }
                        };
                    }
                }
            }
            .with_current_subscriber()
            .instrument(tracing::Span::current()),
        );
        jh
    }
}

use std::time::Duration;

use rpc::{Client as _, ClientError, Transport as _};
use shared_schema::SkyNode;
use tokio::task::{JoinHandle, JoinSet};
use tracing::{Instrument as _, debug, info, info_span, instrument::WithSubscriber as _, trace};

use crate::{
    quinn_transport,
    request_handler::{Error, RootHandler, RootRequest},
};

#[derive(Clone)]
pub struct SkyServer {
    handler: RootHandler<'static>,
    transport: quinn_transport::Transport,
}

impl SkyServer {
    pub async fn new() -> Result<Self, ClientError<quinn_transport::Error, Error>> {
        let tp = quinn_transport::Transport::self_signed_server()
            .await
            .map_err(ClientError::Transport)?;
        let pub_addr = tp.inner().local_addr().unwrap();
        let handler = RootHandler::new(&tp, pub_addr.ip()).await.unwrap();

        Ok(Self {
            handler,
            transport: tp,
        })
    }

    pub fn into_parts(self) -> (RootHandler<'static>, quinn_transport::Transport) {
        (self.handler, self.transport)
    }
    pub async fn add_nodes(&self, nodes: Vec<SkyNode>) {
        self.handler.add_nodes(nodes).await;
    }
    pub async fn bootstrap(&self) {
        self.handler.join_network().await;
    }

    pub fn run_refresh_loop(&self) {
        let handler = self.clone().into_parts().0;
        let local_node = handler.local_node();
        let span = info_span!("refresh_loop", me = ?local_node);
        tokio::task::spawn(
            async move {
                tokio::time::sleep(Duration::from_secs(rand::random_range((5 * 60)..(10 * 60))))
                    .await;
                // tokio::time::sleep(Duration::from_secs(8 * 60)).await;
                loop {
                    info!("refreshing buckets!");
                    handler
                        .refresh_stale_buckets(Duration::from_secs(60 * 65))
                        .await;
                    info!("finished refreshing buckets");
                    tokio::time::sleep(Duration::from_secs(60 * 60)).await;
                }
            }
            .with_current_subscriber()
            .instrument(span),
        );
    }

    pub fn run(&self) -> JoinHandle<Result<(), ClientError<quinn_transport::Error, Error>>> {
        self.run_refresh_loop();

        let (handler, tp) = self.clone().into_parts();

        let jh = tokio::task::spawn(async move {
            let mut js: JoinSet<Result<(), ClientError<quinn_transport::Error, Error>>> =
                JoinSet::new();
            loop {
                trace!("awaiting client");
                let incoming_client: quinn_transport::Incoming =
                    tp.accept().await.map_err(ClientError::Transport)?;
                let handler = handler.clone();
                let span = info_span!("handling request", self_addr = %incoming_client.inner().local_ip().unwrap(), client = %incoming_client.inner().remote_address());
                js.spawn(async move {
                    trace!("accepting {}", incoming_client.inner().remote_address());
                    let conn = match rpc::Incoming::accept(incoming_client).await {
                        Ok(v) => v,
                        Err(e) => {
                            info!("timing out...");
                            Err(e).map_err(ClientError::Transport)?
                        }
                    };
                    debug!("accepted client {}", conn.inner().remote_address());
                    let mut js: JoinSet<_> =
                        JoinSet::new();
                    while conn.inner().close_reason().is_none() {
                        let mut cloned_handler = handler.clone();
                        let conn = conn.clone();
                        let stream = match conn.accept_stream().await {
                            Ok(stream) => stream,
                            Err(e) => {
                                debug!("Err while accepting stream: {}", e);
                                continue;
                            }
                        };
                        js.spawn(async move {
                        if let Err(e) = conn.handle_one_request::<RootRequest, _>(stream, &mut cloned_handler).await {
                            match e {
                                ClientError::Transport(crate::quinn_transport::Error::Connection(
                                    quinn::ConnectionError::ApplicationClosed(close),
                                )) if close.error_code == 0u32.into()
                                    && close.reason.len() == 0 =>
                                {
                                    tracing::warn!("normal client closure")
                                },
                                ClientError::Transport(crate::quinn_transport::Error::Connection(quinn::ConnectionError::TimedOut)) => {
                                    tracing::debug!("connection to server timed out. Okay if the client still got all their responses")
                                }
                                ClientError::Rpc(rpc::RpcError::MinicborIo(minicbor_io::Error::Io(e))) => {
                                    tracing::debug!("probably normal client closure {}", e);
                                }
                                e => tracing::warn!("Error: {e}"),
                            }
                        }
                        });
                    }
                    js.join_all().await;
                    Ok(())
                }.instrument(span));
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
        }.with_current_subscriber().instrument(tracing::Span::current()));
        jh
    }
}

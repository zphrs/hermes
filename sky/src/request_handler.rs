use std::convert::Infallible;

use maxlen::MaxLen;
use rpc::Call;
use shared_schema::ping;

// stands for hermes sky
pub const PORT: u16 = u16::from_be_bytes(*b"hs");

#[derive(Debug, minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
#[cbor(flat)]
pub enum Request {
    /// can be sent by anyone
    #[n(0)]
    Ping(#[n(0)] ping::Request),
}

impl From<ping::Request> for Request {
    fn from(value: ping::Request) -> Self {
        Self::Ping(value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}

impl From<Infallible> for Error {
    fn from(_value: Infallible) -> Self {
        unreachable!()
    }
}

pub struct RootHandler {}

impl rpc::RootHandler<Request> for RootHandler {
    type Error = Error;

    async fn handle<T: futures_io::AsyncWrite + Unpin + Sync + Send, TransportError>(
        &mut self,
        root: Request,
        replier: rpc::Replier<'_, T>,
    ) -> Result<rpc::ReplyReceipt, rpc::ClientError<TransportError, Self::Error>> {
        match root {
            Request::Ping(request) => ping::Method.reply(replier, request).await,
        }
    }
}

#[cfg(test)]
mod tests {

    use futures_util::SinkExt;
    use quinn::ApplicationClose;
    use rand::Rng;
    use std::{cell::RefCell, collections::HashSet, panic, rc::Rc};

    use rpc::{Caller as _, Client as _, ClientError, Close, Incoming, Transport as _};
    use tokio::task::JoinSet;
    use tracing::{Instrument, Level, span, trace, warn};

    use end_to_end_test::{
        Host, Machine, OsShim,
        net::ip,
        sim::{MachineRef, RNG, Sim},
    };

    use crate::{
        quinn::{Client, Transport},
        request_handler::{PORT, RootHandler},
    };

    pub fn create_server() -> MachineRef<OsShim> {
        let server = OsShim::new(Host::new(move || {
            let span = span!(Level::DEBUG, "server");
            async {
                let mut tp = Transport::self_signed_server().await?;
                trace!("inited");
                let mut js: JoinSet<Result<(), ClientError<crate::quinn::Error, super::Error>>> =
                    JoinSet::new();
                let total_packets = Rc::new(RefCell::new(0));
                let dropped_packets = Rc::new(RefCell::new(0));
                let mut successful_conns = 0;
                while successful_conns < 1000 {
                    let total_packets = total_packets.clone();
                    let dropped_packets = dropped_packets.clone();
                    let incoming_client = match tp.accept().await {
                        Ok(c) => c,
                        Err(e) => Err(e)?,
                    };
                    js.spawn_local(async move {
                        trace!("accepted client conn");
                        let mut client = incoming_client
                            .accept()
                            .await
                            .map_err(ClientError::Transport)?;
                        if let Err(e) = client.handle_client(RootHandler {}).await {
                            match e {
                                //Connection(ApplicationClosed(ApplicationClose { error_code: 0, reason: b"" }))
                                ClientError::Transport(crate::quinn::Error::Connection(
                                    quinn::ConnectionError::ApplicationClosed(close),
                                )) if close.error_code == 0u32.into()
                                    && close.reason.len() == 0 =>
                                {
                                    trace!("successfully closed!")
                                }
                                e => Err(e)?,
                            }
                        };
                        *dropped_packets.borrow_mut() += client.stats().path.lost_packets;
                        *total_packets.borrow_mut() += client.stats().path.sent_packets;
                        Ok(())
                    });
                    while let Some(result) = js.try_join_next() {
                        result??;
                    }
                    successful_conns += 1;
                }

                while let Some(result) = js.join_next().await {
                    result??;
                }

                tracing::warn!(
                    "packet loss: {:.2?}%",
                    (*dropped_packets.borrow() as f64 / *total_packets.borrow() as f64) * 100.0
                );

                trace!("joining conns");
                trace!("closed conns");
                warn!("handshake stats: {:#?}", tp.stats());
                tp.close().await;

                trace!("handled client");

                Ok(())
            }
            .instrument(span)
        }));
        server
    }

    pub fn create_client(server_addr: std::net::IpAddr) -> MachineRef<OsShim> {
        OsShim::new(Host::new(move || {
            let span = span!(Level::DEBUG, "client");
            async move {
                let mut tp = Transport::client().await?;
                trace!("inited");
                let base_ms: f64 = 5_000.0;
                let max_ms: f64 = 30_000.0; // 30 second cap
                let mut last_delay_ms: f64 = base_ms;

                let conn = loop {
                    match tp.connect((server_addr, PORT).into()).await {
                        Ok(c) => break c,
                        Err(crate::quinn::Error::Connection(quinn::ConnectionError::TimedOut)) => {
                            // Decorrelated jitter: min(cap, random(base, last_delay * 3))
                            let max_jitter: f64 = (last_delay_ms * 3.0).min(max_ms);
                            let sleep_ms = RNG.with(|rng| {
                                rng.borrow_mut()
                                    .random_range(base_ms as u64..max_jitter as u64)
                                    as f64
                            });
                            last_delay_ms = sleep_ms;
                            let sleep_duration =
                                tokio::time::Duration::from_millis(sleep_ms as u64);
                            trace!("sleeping for {:?}", sleep_duration);
                            tokio::time::sleep(sleep_duration).await;
                            continue;
                        }
                        Err(e) => Err(e)?,
                    }
                };
                let mut js: JoinSet<Result<(), Box<dyn std::error::Error>>> = JoinSet::new();

                for _ in 0..100 {
                    let mut conn_clone = conn.clone();
                    js.spawn_local(async move {
                        conn_clone
                            .query::<shared_schema::ping::Method, super::Request>(
                                shared_schema::ping::Request,
                            )
                            .await
                            .unwrap();
                        Ok(())
                    });
                }

                while let Some(result) = js.join_next().await {
                    result?.unwrap();
                }

                let stats = conn.stats();
                trace!("client stats: {:#?}", stats);
                conn.close().await;
                tp.close().await;

                trace!("got resp");
                Ok(())
            }
            .instrument(span)
        }))
    }

    #[test]
    pub fn ping_test() {
        let _ = env_logger::builder()
            .is_test(true)
            .format_timestamp(None)
            .try_init();
        let sim = Sim::new();
        sim.enter_runtime(|| {
            let net = Sim::add_machine(ip::Network::new());
            let server = create_server();

            let server_addr = server.get().borrow().connect_to_net(net);
            server.get().borrow().set_public_ip(server_addr);
            Sim::tick_machine(server).unwrap();
            let mut arr = vec![server];
            for _ in 0..1000 {
                let client = create_client(server_addr);
                let client_ip = client.get().borrow().connect_to_net(net);
                client.get().borrow().set_public_ip(client_ip);
                arr.push(client);
            }
            let total_machines = arr.len();
            let mut tick_count = 0;
            // loop {
            //     // tick all machines
            //     let mut any_machine_ran = false;
            //     let mut new_array = Vec::new();
            //     // keep track of tick count
            //     tick_count += 1;

            //     for machine in arr.iter() {
            //         if machine.get().borrow().is_idle() {
            //             continue;
            //         }
            //         any_machine_ran = true;
            //         new_array.push(*machine);
            //         Sim::tick_machine(*machine).unwrap();
            //     }
            //     // print progress
            //     if tick_count % 1000 == 0 {
            //         tracing::warn!(
            //             "progress {}/{} (tick_count: {})",
            //             new_array.len(),
            //             total_machines,
            //             tick_count
            //         );
            //     }
            //     arr = new_array;
            //     Sim::tick_machine(net).unwrap();
            //     if !any_machine_ran {
            //         break;
            //     }
            // }
            Sim::run_until_idle(|| arr.iter()).unwrap();
        })
    }
}

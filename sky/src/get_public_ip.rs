use std::{net::IpAddr, sync::Arc};

use tokio::{
    net::UdpSocket,
    task::{JoinSet, LocalSet},
};
use tracing::{info, trace};

// this pings out to STUN servers to get IP. Also pings dns and api providers like ipify et. al.
pub async fn get_public_ip() -> Option<IpAddr> {
    use futures_util::StreamExt;
    use std::collections::HashMap;
    let ls: LocalSet = LocalSet::new();
    let jh1 = ls.spawn_local(
        public_ip::resolve(public_ip::http::ALL, public_ip::Version::Any)
            .filter_map(|v| async move { v.ok().map(|v| v.0) })
            .collect(),
    );
    let jh2 = ls.spawn_local(query_stun_server());

    let ip_addrs: Vec<_> = ls.run_until(jh1).await.unwrap_or_default();
    let ip_addrs_2: Vec<_> = ls.run_until(jh2).await.unwrap_or_default();
    // return most frequent from all sources who returned as a "best guess" for pub ip.
    // this way any ip address provider can't create a DOS
    let mut hm: HashMap<IpAddr, usize> = HashMap::new();
    for ip_addr in ip_addrs.into_iter().chain(ip_addrs_2) {
        *hm.entry(ip_addr).or_default() += 1
    }
    let out = hm.into_iter().max_by_key(|v| v.1).map(|v| v.0);
    info!("got public ip {ip:?}", ip = out);
    out
}
#[cfg(test)]
pub async fn get_public_ip_mock() -> Option<IpAddr> {
    use end_to_end_test::{OsShim, sim::Sim};
    let curr_os = Sim::get_current_machine::<OsShim>();
    trace!("got current os shim address");
    curr_os.borrow().public_ip()
}

// returns first ip returned from any of the default stun servers
async fn query_stun_server() -> Vec<IpAddr> {
    static DEFAULT_STUN_SERVERS: [&'static str; 4] = [
        "stun.cloudflare.com",
        "stun.l.google.com",
        "stun.syncthing.net",
        "stun.xten.com",
    ];
    use stun::{
        agent::TransactionId,
        client::ClientBuilder,
        message::{BINDING_REQUEST, Getter, Message},
        xoraddr::XorMappedAddress,
    };
    let ls = LocalSet::new();
    ls.run_until(async {
        let mut js: JoinSet<Result<IpAddr, stun::Error>> = JoinSet::new();
        for addr in DEFAULT_STUN_SERVERS {
            js.spawn_local(async move {
                let (handler_tx, mut handler_rx) = tokio::sync::mpsc::unbounded_channel();

                let conn = UdpSocket::bind("0.0.0.0:0").await?;
                trace!("Local address: {}", conn.local_addr()?);

                trace!("Connecting to: {conn:?}");
                conn.connect(format!("{addr}:3478")).await?;

                let mut client = ClientBuilder::new().with_conn(Arc::new(conn)).build()?;

                let mut msg = Message::new();
                msg.build(&[Box::<TransactionId>::default(), Box::new(BINDING_REQUEST)])?;

                client.send(&msg, Some(Arc::new(handler_tx))).await?;

                let xor_addr = {
                    if let Some(event) = handler_rx.recv().await {
                        let msg = event.event_body?;
                        let mut xor_addr = XorMappedAddress::default();
                        xor_addr.get_from(&msg)?;
                        trace!("Got response: {xor_addr}");
                        Ok(xor_addr.ip)
                    } else {
                        Err(stun::Error::ErrNoDefaultReason)
                    }
                };

                let _ = client.close().await;
                Ok(xor_addr.unwrap())
            });
        }
        let all_addrs: Vec<_> = js
            .join_all()
            .await
            .into_iter()
            .filter_map(|v| v.ok())
            .collect();
        all_addrs
    })
    .await
}
#[cfg(test)]
mod tests {
    use crate::get_public_ip::get_public_ip;

    #[tokio::test]
    #[test_log::test]
    async fn pub_ip() {
        println!("{:?}", get_public_ip().await);
    }
}

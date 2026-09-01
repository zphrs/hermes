mod a_to_b;
mod loopback;
mod race;

use std::{net::SocketAddr, time::Duration};

use anyhow::anyhow;
use dens::{MachineIntoRef, Sim};
use tracing::{Instrument, info_span};

use crate::quinn_transport::create_endpoint::{dens_client, dens_server};

async fn accept_client(endpoint: &quinn::Endpoint) -> anyhow::Result<quinn::Connection> {
    Ok(endpoint
        .accept()
        .await
        .ok_or_else(|| anyhow!("endpoint closed"))?
        .await?)
}

async fn connect_to_server(
    endpoint: &quinn::Endpoint,
    address: impl Into<SocketAddr>,
) -> anyhow::Result<quinn::Connection> {
    Ok(endpoint.connect(address.into(), "server.invalid")?.await?)
}

/// spawns one client and one server with [`quinn::Endpoint`]s that share
/// a network.
fn harness<
    ClientFut: Future<Output = anyhow::Result<()>>,
    ServerFut: Future<Output = anyhow::Result<()>>,
>(
    client: impl Fn(quinn::Endpoint, SocketAddr) -> ClientFut + 'static + Copy,
    server: impl Fn(quinn::Endpoint) -> ServerFut + 'static + Copy,
) -> Result<(), anyhow::Error> {
    Sim::new_with_config(dens::sim::Config {
        tick_amount: Duration::from_millis(10),
        ..Default::default()
    })
    .enter_runtime(|| {
        let net = dens::IpNetwork::new_private_class_c().into_ref();
        let server = dens::os_mock::OsMock::new(move || {
            async move { server(dens_server(8000).await?).await }.instrument(info_span!("server"))
        });

        let (server_ipv4, _) = server.connect_to_net(net);
        let server = Sim::add_machine(server);

        let client = dens::os_mock::OsMock::new(move || {
            async move { client(dens_client().await?, (server_ipv4, 8000).into()).await }
                .instrument(info_span!("client"))
        });
        client.connect_to_net(net);
        let client = client.into_ref();

        let arr = [client, server];

        Sim::run_until_idle(|| arr.iter())?;

        anyhow::Ok(())
    })
}

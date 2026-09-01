use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};

use anyhow::anyhow;
use dens::{MachineIntoRef, Sim};

use minicbor::bytes::ByteSlice;
use quinn::EndpointConfig;
use tracing::{Instrument, info, info_span, instrument, trace, warn};

use crate::{
    io::request,
    quinn_transport,
    traits::{
        markers::False,
        method::{
            self,
            handler::{
                replier::futures_io,
                root_method::{RootHandler, RootMethod},
            },
        },
    },
};

pub struct Ping;

impl method::Method for Ping {
    type Req<'buf> = &'buf minicbor::bytes::ByteSlice;

    type Res<'buf> = &'buf minicbor::bytes::ByteSlice;

    type Transitions = False;

    type HasDescendants = False;
}

impl method::handler::LeafHandler for Ping {
    async fn handle<'a>(&mut self, request: method::ReqOf<'a, Self>) -> method::ResOf<'a, Self> {
        request
    }
}

#[instrument(skip(connection))]
async fn server_handle_connection(connection: quinn::Connection) -> anyhow::Result<()> {
    trace!("accepted connection");
    let mut read_buf = Vec::new();
    loop {
        let (send, recv) = connection.accept_bi().await?;
        trace!("accepted bi");
        let replier = futures_io::Replier::<RootMethod<Ping>, _>::new(send);

        crate::io::respond(&mut read_buf, recv, replier, &mut RootHandler(Ping)).await?;
    }
}

#[test_log::test]
fn ping() -> anyhow::Result<()> {
    let sim = dens::Sim::new();
    sim.enter_runtime(|| {
        let net = dens::IpNetwork::new_private_class_c().into_ref();

        let server = dens::os_mock::OsMock::new(|| {
            async {
                trace!("running");
                let socket = dens::os_mock::net::UdpSocket::bind(SocketAddr::new(
                    Ipv4Addr::UNSPECIFIED.into(),
                    8000,
                ))
                .await?;
                let wrapped =
                    crate::quinn_transport::end_to_end_socket::EndToEndSocket::from(socket);
                let (server_config, _cert) = crate::quinn_transport::server::configure_server()?;
                let endpoint = quinn::Endpoint::new_with_abstract_socket(
                    EndpointConfig::default(),
                    Some(server_config),
                    Arc::new(wrapped),
                    Arc::new(quinn_transport::dens_runtime::DensRuntime),
                )?;
                loop {
                    let connection = endpoint
                        .accept()
                        .await
                        .ok_or_else(|| anyhow!("endpoint closed"))?
                        .await?;
                    let remote_addr = connection.remote_address();
                    if let Err(err) = server_handle_connection(connection).await {
                        warn!("error while handling connection from {remote_addr}: {err}");
                    }
                }
            }
            .instrument(info_span!("server"))
        });
        let (server_ip, _) = server.connect_to_net(net);
        info!("server ip: {server_ip}");
        Sim::add_machine(server);

        let client = dens::os_mock::OsMock::new(move || {
            async move {
                trace!("running");
                let socket = dens::os_mock::net::UdpSocket::bind(SocketAddr::new(
                    Ipv4Addr::UNSPECIFIED.into(),
                    0,
                ))
                .await?;
                let wrapped =
                    crate::quinn_transport::end_to_end_socket::EndToEndSocket::from(socket);
                let client_config = quinn_transport::client::configure_client()?;

                let mut endpoint = quinn::Endpoint::new_with_abstract_socket(
                    EndpointConfig::default(),
                    None,
                    Arc::new(wrapped),
                    Arc::new(quinn_transport::dens_runtime::DensRuntime),
                )?;

                endpoint.set_default_client_config(client_config);
                trace!("connecting to server");

                let mut buf = Vec::new();
                let connection = endpoint
                    .connect(SocketAddr::from((server_ip, 8000)), "server.invalid")?
                    .await?;
                trace!("connected to server");

                let stream = connection.open_bi().await?;

                let req: &'static ByteSlice = b"Hello, world!".as_slice().into();

                let res =
                    request::<RootMethod<Ping>, Ping, quinn::Connection>(&mut buf, req, stream)
                        .await?;

                assert_eq!(res, req);

                Ok(())
            }
            .instrument(info_span!("client"))
        });
        client.connect_to_net(net);
        let client = Sim::add_machine(client);

        let arr = [client];
        Sim::run_until_idle(|| arr.iter())
    })?;
    Ok(())
}

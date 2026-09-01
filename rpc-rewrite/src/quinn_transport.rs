pub mod client;
pub mod end_to_end_socket;
pub mod server;
pub mod skip_server_verification;
pub mod dens_runtime {
    use quinn::TokioRuntime;

    #[derive(Debug)]
    pub struct DensRuntime;

    impl quinn::Runtime for DensRuntime {
        fn new_timer(&self, i: std::time::Instant) -> std::pin::Pin<Box<dyn quinn::AsyncTimer>> {
            TokioRuntime.new_timer(i)
        }

        fn spawn(&self, future: std::pin::Pin<Box<dyn Future<Output = ()> + Send>>) {
            TokioRuntime.spawn(future)
        }

        fn wrap_udp_socket(
            &self,
            _t: std::net::UdpSocket,
        ) -> std::io::Result<std::sync::Arc<dyn quinn::AsyncUdpSocket>> {
            unimplemented!()
        }
    }
}

pub mod create_endpoint {
    use std::{
        net::{Ipv4Addr, SocketAddr},
        sync::Arc,
    };

    use quinn::EndpointConfig;

    use crate::quinn_transport::{client, dens_runtime, end_to_end_socket::EndToEndSocket, server};
    fn create(socket: impl Into<EndToEndSocket>) -> anyhow::Result<quinn::Endpoint> {
        let config = EndpointConfig::default();
        Ok(quinn::Endpoint::new_with_abstract_socket(
            config,
            None,
            Arc::new(socket.into()),
            Arc::new(dens_runtime::DensRuntime),
        )?)
    }

    fn dens_client_inner(socket: impl Into<EndToEndSocket>) -> anyhow::Result<quinn::Endpoint> {
        let mut endpoint = create(socket)?;

        endpoint.set_default_client_config(client::configure_client()?);
        Ok(endpoint)
    }
    pub async fn dens_client() -> anyhow::Result<quinn::Endpoint> {
        let socket =
            dens::os_mock::net::UdpSocket::bind(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0))
                .await?;
        dens_client_inner(socket)
    }

    fn dens_server_inner(socket: impl Into<EndToEndSocket>) -> anyhow::Result<quinn::Endpoint> {
        let endpoint = create(socket)?;

        endpoint.set_server_config(Some(server::configure_server()?.0));
        Ok(endpoint)
    }

    pub async fn dens_server(port: u16) -> anyhow::Result<quinn::Endpoint> {
        let socket = dens::os_mock::net::UdpSocket::bind(SocketAddr::new(
            Ipv4Addr::UNSPECIFIED.into(),
            port,
        ))
        .await?;

        let endpoint = dens_server_inner(socket)?;

        Ok(endpoint)
    }
}

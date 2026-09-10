use minicbor::{CborLen, Decode, Encode};

use super::Connection;
use super::read::{self, read};
use crate::method::{Notification, ReqOf};

mod error {
    use super::Connection;

    /// notification follows these steps:
    /// 1. open unidirectional stream
    /// 2. [`Write`] notification
    #[derive(thiserror::Error)]
    pub enum SendError<C: Connection> {
        #[error("stream could not be opened")]
        Open(#[source] C::OpenUniError),
        #[error("could not write notification")]
        Write(#[from] super::super::write::Error<C::SendStream>),
    }

    impl<C: Connection> std::fmt::Debug for SendError<C>
    where
        C::OpenUniError: std::fmt::Debug,
    {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Open(arg0) => f.debug_tuple("Open").field(arg0).finish(),
                Self::Write(arg0) => f.debug_tuple("Write").field(arg0).finish(),
            }
        }
    }
    /// 1. accept unidirectional stream
    /// 2. [`Read`] notification
    #[derive(thiserror::Error)]
    pub enum RecvError<C: Connection> {
        #[error("could not accept stream")]
        Accept(#[source] C::AcceptUniError),
        #[error("could not read notification")]
        Read(#[from] super::read::Error<C::RecvStream>),
    }

    impl<C: Connection> std::fmt::Debug for RecvError<C>
    where
        C::AcceptUniError: std::fmt::Debug,
    {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Accept(arg0) => f.debug_tuple("Accept").field(arg0).finish(),
                Self::Read(arg0) => f.debug_tuple("Read").field(arg0).finish(),
            }
        }
    }
}
pub use error::RecvError;
pub use error::SendError;

pub async fn send<'request, RootMethod: Notification, C: Connection>(
    request: ReqOf<'request, RootMethod>,
    connection: &C,
) -> Result<(), SendError<C>>
where
    ReqOf<'request, RootMethod>: CborLen<()> + Encode<()>,
{
    let send = connection
        .open_uni_stream()
        .await
        .map_err(SendError::Open)?;

    super::write::write::<RootMethod::Req<'request>, _>(&request, send).await?;

    Ok(())
}

pub async fn receive<'buf, RootMethod: Notification, C: Connection>(
    buf: &'buf mut Vec<u8>,
    conn: &C,
) -> Result<ReqOf<'buf, RootMethod>, RecvError<C>>
where
    ReqOf<'buf, RootMethod>: Decode<'buf, ()>,
{
    let recv = conn.accept_uni_stream().await.map_err(RecvError::Accept)?;
    buf.clear();
    Ok(read::<RootMethod::Req<'buf>, _>(buf, recv).await?)
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use anyhow::anyhow;
    use dens::{MachineIntoRef, Sim};
    use tracing::{info, trace};

    use crate::{
        Method,
        io::notify,
        marker::{False, NotApplicable},
        quinn_transport::create_endpoint,
    };

    pub struct Notify;

    impl Method for Notify {
        type Req<'buf> = ();

        type Res<'buf> = NotApplicable;

        type Transitions = False;

        type HasDescendants = False;
    }

    async fn server(connection: quinn::Connection) -> anyhow::Result<()> {
        let () = notify::receive::<Notify, _>(&mut Vec::new(), &connection).await?;
        Ok(())
    }

    async fn client(connection: quinn::Connection) -> anyhow::Result<()> {
        let () = notify::send::<Notify, _>((), &connection).await?;
        // need to wait until connection closes fully otherwise notification
        // message and connection close message (from dropping connection)
        // can race on the wire, resulting in the server potentially
        // receiving the close method before the notification message which
        // leads to erroring out the [`notify::receive`].
        connection.closed().await;
        Ok(())
    }

    async fn server_setup(port: u16) -> anyhow::Result<quinn::Connection> {
        let endpoint = create_endpoint::dens_server(port).await?;

        let connection = endpoint
            .accept()
            .await
            .ok_or_else(|| anyhow!("endpoint closed"))?
            .await?;
        Ok(connection)
    }

    async fn client_setup(
        server_address: impl Into<SocketAddr>,
    ) -> anyhow::Result<quinn::Connection> {
        let endpoint = create_endpoint::dens_client().await?;

        trace!("connecting to server");

        let connection = endpoint
            .connect(server_address.into(), "server.invalid")?
            .await?;

        Ok(connection)
    }

    #[test]
    fn test() -> anyhow::Result<()> {
        Sim::new().enter_runtime(|| {
            let net = dens::IpNetwork::new_private_class_c().into_ref();
            let server =
                dens::os_mock::OsMock::new(|| async { server(server_setup(8000).await?).await });

            let (server_ipv4, _) = server.connect_to_net(net);
            info!("server ip: {server_ipv4}");
            Sim::add_machine(server);

            let client = dens::os_mock::OsMock::new(move || async move {
                let connection = client_setup((server_ipv4, 8000)).await?;
                client(connection).await
            });
            client.connect_to_net(net);
            let client = client.into_ref();

            let arr = [client];

            Sim::run_until_idle(|| arr.iter())?;

            anyhow::Ok(())
        })
    }
}

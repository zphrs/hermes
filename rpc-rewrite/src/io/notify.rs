use std::convert::Infallible;

use bytes::Bytes;
use minicbor::{CborLen, Decode, Encode};

use crate::{
    io::{read_to_end, write_all},
    traits::{
        Connection, Method,
        io::{BytesReadStream, BytesWriteStream, Stopped},
        method::{Notification, ReqOf},
    },
};

#[derive(Debug, thiserror::Error)]
pub enum SendError<C: Connection> {
    #[error("encode: {0}")]
    Encode(#[from] minicbor::encode::Error<Infallible>),
    #[error("io: {0}")]
    Write(<C::SendStream as BytesWriteStream>::Error),
    #[error("open stream: {0}")]
    Open(C::OpenUniError),
}

#[derive(thiserror::Error)]
pub enum ReceiveError<C: Connection> {
    #[error("encode: {0}")]
    Decode(#[from] minicbor::decode::Error),
    #[error("io: {0}")]
    Read(<C::RecvStream as BytesReadStream>::Error),
    #[error("accept stream: {0}")]
    Accept(C::AcceptUniError),
}

impl<C: Connection> std::fmt::Debug for ReceiveError<C>
where
    C::AcceptUniError: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode(arg0) => f.debug_tuple("Decode").field(arg0).finish(),
            Self::Read(arg0) => f.debug_tuple("Read").field(arg0).finish(),
            Self::Accept(arg0) => f.debug_tuple("Accept").field(arg0).finish(),
        }
    }
}

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

    let mut buf = Vec::with_capacity(minicbor::len(&request));
    minicbor::encode(request, &mut buf)?;
    write_all(send, Bytes::from(buf), core::cfg!(test))
        .await
        .map_err(SendError::Write)?;

    Ok(())
}

pub async fn receive<'buf, RootMethod: Method, C: Connection>(
    buf: &'buf mut Vec<u8>,
    conn: &C,
) -> Result<ReqOf<'buf, RootMethod>, ReceiveError<C>>
where
    ReqOf<'buf, RootMethod>: Decode<'buf, ()>,
{
    let recv = conn
        .accept_uni_stream()
        .await
        .map_err(ReceiveError::Accept)?;
    buf.clear();
    read_to_end(recv, buf, usize::MAX)
        .await
        .map_err(ReceiveError::Read)?;
    Ok(minicbor::decode(buf)?)
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use anyhow::anyhow;
    use dens::{MachineIntoRef, Sim};
    use tracing::{info, trace};

    use crate::{
        io::notify,
        quinn_transport::create_endpoint,
        traits::{
            Method,
            markers::{False, NotApplicable},
        },
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

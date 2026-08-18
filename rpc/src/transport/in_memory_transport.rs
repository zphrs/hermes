//! 100% reliable in-memory transport intended for local testing.

mod setup_conn;

pub use setup_conn::{ConnPair, setup_conn};

use std::{
    collections::HashMap,
    convert::Infallible,
    future::Future,
    hash::Hash,
    pin::pin,
    sync::{Arc, Mutex},
    task::{Poll, ready},
};

use bytes::Bytes;
use futures::{AsyncRead, FutureExt};
use tokio::sync::mpsc::Permit;

type StreamChannelTx = tokio::sync::mpsc::Sender<(SendStream, RecvStream)>;
type StreamChannelRx = tokio::sync::mpsc::Receiver<(SendStream, RecvStream)>;
type IncomingChannelTx<Address> =
    tokio::sync::mpsc::Sender<(Address, StreamChannelRx, StreamChannelTx)>;
type IncomingChannelRx<Address> =
    tokio::sync::mpsc::Receiver<(Address, StreamChannelRx, StreamChannelTx)>;

#[derive(Clone)]
pub struct Network<Address = [u8; 16]> {
    registry: Arc<Mutex<HashMap<Address, IncomingChannelTx<Address>>>>,
}

impl<Address> Network<Address>
where
    Address: Eq + Hash + Copy,
{
    pub fn new() -> Self {
        Self {
            registry: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn new_transport(&self, address: Address) -> MemoryTransport<Address> {
        let (tx, rx) = tokio::sync::mpsc::channel(1024);
        self.registry.lock().unwrap().insert(address, tx);

        MemoryTransport {
            address,
            network: self.clone(),
            incoming_rx: Arc::new(tokio::sync::Mutex::new(rx)),
        }
    }
}

impl<Address> Default for Network<Address>
where
    Address: Eq + Hash + Copy,
{
    fn default() -> Self {
        Self::new()
    }
}

/// 100% reliable in-memory transport intended for local testing.
#[derive(Clone)]
pub struct MemoryTransport<Address = [u8; 16]> {
    address: Address,
    network: Network<Address>,
    incoming_rx: Arc<tokio::sync::Mutex<IncomingChannelRx<Address>>>,
}

impl<Address> MemoryTransport<Address>
where
    Address: Eq + Hash + Copy,
{
    pub fn new(network: Network<Address>, address: Address) -> Self {
        network.new_transport(address)
    }

    pub fn address(&self) -> Address {
        self.address
    }
}

impl<Address> crate::transport::Transport for MemoryTransport<Address>
where
    Address: Eq + Hash + Copy,
{
    type Address = Address;
    type Error = Infallible;
    type Caller = Connection<Address>;
    #[inline(never)]
    fn connect(
        &self,
        to: &Self::Address,
    ) -> impl Future<Output = Result<Self::Caller, Self::Error>> {
        let tx = self
            .network
            .registry
            .lock()
            .unwrap()
            .get(to)
            .cloned()
            .expect("Peer not found");
        let local_addr = self.address;
        let remote_addr = *to;
        async move {
            // caller -> server channel (caller opens streams to server)
            let (c2s_stream_tx, c2s_stream_rx) = tokio::sync::mpsc::channel(1024);
            // server -> caller channel (server opens streams to caller)
            let (s2c_stream_tx, s2c_stream_rx) = tokio::sync::mpsc::channel(1024);
            let _ = tx.send((local_addr, c2s_stream_rx, s2c_stream_tx)).await;
            Ok(Connection {
                stream_tx: c2s_stream_tx,
                stream_rx: Arc::new(tokio::sync::Mutex::new(s2c_stream_rx)),
                remote_addr,
                local_addr,
            })
        }
    }

    type Client = Connection<Address>;
    type Incoming = Incoming<Address>;

    fn accept(&self) -> impl Future<Output = Result<Self::Incoming, Self::Error>> {
        let rx = self.incoming_rx.clone();
        let local_addr = self.address;
        async move { Ok(Incoming { rx, local_addr }) }
    }
}

#[derive(Clone, Debug)]
pub struct Connection<Address = [u8; 16]> {
    stream_tx: StreamChannelTx,
    stream_rx: Arc<tokio::sync::Mutex<StreamChannelRx>>,
    remote_addr: Address,
    local_addr: Address,
}

impl<T: PartialEq> PartialEq for Connection<T> {
    fn eq(&self, other: &Self) -> bool {
        self.remote_addr == other.remote_addr && self.local_addr == other.local_addr
    }
}

impl<Address> Connection<Address> {
    pub fn remote_addr(&self) -> &Address {
        &self.remote_addr
    }

    pub fn local_addr(&self) -> &Address {
        &self.local_addr
    }
}

impl<Address> crate::transport::BiStream for Connection<Address> {
    type RecvStream = RecvStream;
    type SendStream = SendStream;
}

impl<Address> crate::transport::Client for Connection<Address> {
    type Error = std::io::Error;
    #[inline(never)]
    fn accept_stream(&self) -> impl Future<Output = Result<(SendStream, RecvStream), Self::Error>> {
        Box::pin(async move {
            self.stream_rx
                .clone()
                .lock_owned()
                .await
                .recv()
                .await
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))
        })
    }
}

pub struct OpenStreamFut {
    client: Option<(SendStream, RecvStream)>,
    caller: Option<(SendStream, RecvStream)>,
    stream_tx: tokio::sync::mpsc::Sender<(SendStream, RecvStream)>,
}

impl OpenStreamFut {
    pub fn new(stream_tx: tokio::sync::mpsc::Sender<(SendStream, RecvStream)>) -> Self {
        let (c2s_tx, c2s_rx) = tokio::sync::mpsc::channel(1024);
        let (s2c_tx, s2c_rx) = tokio::sync::mpsc::channel(1024);
        let caller_send = SendStream { inner: c2s_tx };
        let caller_recv = RecvStream {
            inner: s2c_rx,
            leftover_bytes: Bytes::new(),
        };

        let client_send = SendStream { inner: s2c_tx };
        let client_recv = RecvStream {
            inner: c2s_rx,
            leftover_bytes: Bytes::new(),
        };

        Self {
            client: Some((client_send, client_recv)),
            caller: Some((caller_send, caller_recv)),
            stream_tx,
        }
    }
}

impl Future for OpenStreamFut {
    type Output = Result<(SendStream, RecvStream), Infallible>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Self::Output> {
        let this = &mut *self;
        let permit: Permit<'_, (SendStream, RecvStream)> =
            match ready!(pin!(this.stream_tx.reserve()).poll_unpin(cx)) {
                Ok(permit) => permit,
                // capacity error
                Err(_e) => return Poll::Pending,
            };
        permit.send(this.client.take().unwrap());
        Poll::Ready(Ok(this.caller.take().unwrap()))
    }
}

impl<Address> crate::transport::Caller for Connection<Address> {
    type Error = Infallible;
    type OpenStreamFut = OpenStreamFut;
    #[inline(never)]
    fn open_stream(&self) -> OpenStreamFut {
        OpenStreamFut::new(self.stream_tx.clone())
    }
}

pub struct RecvStream {
    inner: tokio::sync::mpsc::Receiver<bytes::Bytes>,
    leftover_bytes: bytes::Bytes,
}

impl AsyncRead for RecvStream {
    #[inline(never)]
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let Some(bytes) = ({
            if self.leftover_bytes.is_empty() {
                ready!(self.inner.poll_recv(cx))
            } else {
                Some(self.leftover_bytes.clone())
            }
        }) else {
            return Poll::Ready(Err(std::io::ErrorKind::BrokenPipe)?);
        };

        let copied_len = buf.len().min(bytes.len());
        buf[..copied_len].copy_from_slice(&bytes[..copied_len]);
        self.leftover_bytes = bytes.slice(copied_len..bytes.len());
        Poll::Ready(Ok(copied_len))
    }
}

#[derive(Clone)]
pub struct SendStream {
    inner: tokio::sync::mpsc::Sender<bytes::Bytes>,
}

impl futures::AsyncWrite for SendStream {
    #[inline(never)]
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let owned_buf = bytes::BytesMut::from(buf).freeze();
        if self.inner.try_send(owned_buf).is_err() {
            Poll::Pending
        } else {
            Poll::Ready(Ok(buf.len()))
        }
    }
    #[inline(never)]
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    #[inline(never)]
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.inner.is_closed() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

pub struct Incoming<Address = [u8; 16]> {
    rx: Arc<tokio::sync::Mutex<IncomingChannelRx<Address>>>,
    local_addr: Address,
}

impl<Address> crate::transport::Incoming for Incoming<Address> {
    type Client = Connection<Address>;
    type Error = std::io::Error;

    #[inline(never)]
    async fn accept(self) -> Result<Self::Client, Self::Error> {
        let (remote_addr, stream_rx, stream_tx) = self
            .rx
            .lock_owned()
            .await
            .recv()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))?;
        Ok(Connection {
            stream_tx,
            stream_rx: Arc::new(tokio::sync::Mutex::new(stream_rx)),
            remote_addr,
            local_addr: self.local_addr,
        })
    }
}

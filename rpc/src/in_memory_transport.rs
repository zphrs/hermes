use std::{
    collections::HashMap,
    convert::Infallible,
    hash::Hash,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    task::{Poll, ready},
};

use bytes::Bytes;
use futures::AsyncRead;

type StreamChannelTx = tokio::sync::mpsc::Sender<(SendStream, RecvStream)>;
type StreamChannelRx = tokio::sync::mpsc::Receiver<(SendStream, RecvStream)>;
type IncomingChannelTx<Address> = tokio::sync::mpsc::Sender<(Address, StreamChannelRx)>;
type IncomingChannelRx<Address> = tokio::sync::mpsc::Receiver<(Address, StreamChannelRx)>;

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

/// 100% reliable transport
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
    Address: Eq + Hash + Copy + std::marker::Sync + std::marker::Send,
{
    type Address = Address;
    type Error = Infallible;
    type Caller = Caller<Address>;

    async fn connect(&self, to: &Self::Address) -> Result<Self::Caller, Self::Error> {
        let tx = self
            .network
            .registry
            .lock()
            .unwrap()
            .get(to)
            .cloned()
            .expect("Peer not found");
        let (stream_tx, stream_rx) = tokio::sync::mpsc::channel(1024);
        let _ = tx.send((self.address, stream_rx)).await;
        Ok(Caller {
            stream_tx,
            remote_addr: *to,
            local_addr: self.address,
        })
    }

    type Client = Client<Address>;
    type Incoming = Incoming<Address>;

    async fn accept(&self) -> Result<Self::Incoming, Self::Error> {
        Ok(Incoming {
            rx: self.incoming_rx.clone(),
            local_addr: self.address,
        })
    }
}

pub struct Client<Address = [u8; 16]> {
    stream_rx: Arc<tokio::sync::Mutex<StreamChannelRx>>,
    remote_addr: Address,
    local_addr: Address,
}

impl<Address> Client<Address> {
    pub fn remote_addr(&self) -> &Address {
        &self.remote_addr
    }

    pub fn local_addr(&self) -> &Address {
        &self.local_addr
    }
}

impl<Address> crate::transport::BiStream for Client<Address> {
    type RecvStream = RecvStream;
    type SendStream = SendStream;
}

impl<Address: Send + Sync> crate::transport::Client for Client<Address> {
    type Error = std::io::Error;

    async fn accept_stream(&self) -> Result<(Self::SendStream, Self::RecvStream), Self::Error> {
        let res = self
            .stream_rx
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))?;
        Ok(res)
    }
}

pub struct Caller<Address = [u8; 16]> {
    stream_tx: StreamChannelTx,
    remote_addr: Address,
    local_addr: Address,
}

impl<Address> Caller<Address> {
    pub fn remote_addr(&self) -> &Address {
        &self.remote_addr
    }

    pub fn local_addr(&self) -> &Address {
        &self.local_addr
    }
}

impl<Address> crate::transport::BiStream for Caller<Address> {
    type RecvStream = RecvStream;
    type SendStream = SendStream;
}

impl<Address: Send + Sync> crate::transport::Caller for Caller<Address> {
    type Error = Infallible;

    async fn open_stream(&self) -> Result<(Self::SendStream, Self::RecvStream), Self::Error> {
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

        // Send the client's halves over the connection channel
        let _ = self.stream_tx.send((client_send, client_recv)).await;

        Ok((caller_send, caller_recv))
    }
}

pub struct RecvStream {
    inner: tokio::sync::mpsc::Receiver<bytes::Bytes>,
    leftover_bytes: bytes::Bytes,
}

impl AsyncRead for RecvStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let Some(bytes) = ({
            if self.leftover_bytes.len() > 0 {
                Some(self.leftover_bytes.clone())
            } else {
                ready!(self.inner.poll_recv(cx))
            }
        }) else {
            return Poll::Ready(Err(std::io::ErrorKind::BrokenPipe)?);
        };

        let copied_len = buf.len().min(bytes.len());
        buf[..copied_len].copy_from_slice(&bytes[..copied_len]);
        self.leftover_bytes = bytes.slice(copied_len..bytes.len());
        return Poll::Ready(Ok(copied_len));
    }
}

#[derive(Clone)]
pub struct SendStream {
    inner: tokio::sync::mpsc::Sender<bytes::Bytes>,
}

impl futures::AsyncWrite for SendStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let owned_buf = bytes::BytesMut::from(buf).freeze();
        if let Err(_) = self.inner.try_send(owned_buf) {
            Poll::Pending
        } else {
            Poll::Ready(Ok(buf.len()))
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

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

impl<Address: Send + Sync> crate::transport::Incoming for Incoming<Address> {
    type Client = Client<Address>;
    type Error = std::io::Error;

    async fn accept(self) -> Result<Self::Client, Self::Error> {
        let (remote_addr, stream_rx) = self
            .rx
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))?;
        Ok(Client {
            stream_rx: Arc::new(tokio::sync::Mutex::new(stream_rx)),
            remote_addr,
            local_addr: self.local_addr,
        })
    }
}

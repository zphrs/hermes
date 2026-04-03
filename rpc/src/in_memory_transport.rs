use crate::Transport;
use std::{
    collections::HashMap,
    convert::Infallible,
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

#[derive(Clone)]
pub struct Network {
    registry: Arc<Mutex<HashMap<[u8; 32], tokio::sync::mpsc::Sender<StreamChannelRx>>>>,
    next_id: Arc<AtomicU64>,
}

impl Network {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn new_transport(&self) -> MemoryTransport {
        let mut address = [0u8; 32];
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        address[0..8].copy_from_slice(&id.to_le_bytes());

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
pub struct MemoryTransport {
    address: [u8; 32],
    network: Network,
    incoming_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<StreamChannelRx>>>,
}

impl MemoryTransport {
    pub fn new(network: Network) -> Self {
        network.new_transport()
    }

    pub fn address(&self) -> [u8; 32] {
        self.address
    }
}

impl Transport for MemoryTransport {
    type Address = [u8; 32];
    type Error = Infallible;
    type Caller = Caller;

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
        let _ = tx.send(stream_rx).await;
        Ok(Caller { stream_tx })
    }

    type Client = Client;
    type Incoming = Incoming;

    async fn accept(&self) -> Result<Self::Incoming, Self::Error> {
        Ok(Incoming {
            rx: self.incoming_rx.clone(),
        })
    }
}

pub struct Client {
    stream_rx: Arc<tokio::sync::Mutex<StreamChannelRx>>,
}

impl crate::BiStream for Client {
    type RecvStream = RecvStream;
    type SendStream = SendStream;
}

impl crate::Client for Client {
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

pub struct Caller {
    stream_tx: StreamChannelTx,
}

impl crate::BiStream for Caller {
    type RecvStream = RecvStream;
    type SendStream = SendStream;
}

impl crate::Caller for Caller {
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

pub struct Incoming {
    rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<StreamChannelRx>>>,
}

impl crate::Incoming for Incoming {
    type Client = Client;
    type Error = std::io::Error;

    async fn accept(self) -> Result<Self::Client, Self::Error> {
        let stream_rx = self
            .rx
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))?;
        Ok(Client {
            stream_rx: Arc::new(tokio::sync::Mutex::new(stream_rx)),
        })
    }
}

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

#[cfg(test)]
pub use end_to_end_test::UdpSocket;
#[cfg(not(test))]
pub use tokio::net::UdpSocket;

use quinn::{AsyncUdpSocket, UdpPoller};

#[derive(Debug)]
pub struct EndToEndSocket {
    sock: Mutex<UdpSocket>,
}

impl From<UdpSocket> for EndToEndSocket {
    fn from(sock: UdpSocket) -> Self {
        Self { sock: sock.into() }
    }
}

impl AsyncUdpSocket for EndToEndSocket {
    fn create_io_poller(self: Arc<Self>) -> std::pin::Pin<Box<dyn quinn::UdpPoller>> {
        Box::pin(PollWritable { sock: self })
    }

    fn try_send(&self, transmit: &quinn::udp::Transmit) -> std::io::Result<()> {
        self.sock
            .lock()
            .unwrap()
            .try_send_to(&transmit.contents, transmit.destination)
            .map(|_| ())
    }

    fn poll_recv(
        &self,
        cx: &mut std::task::Context,
        bufs: &mut [std::io::IoSliceMut<'_>],
        meta: &mut [quinn::udp::RecvMeta],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if bufs.is_empty() || meta.is_empty() {
            return std::task::Poll::Ready(Ok(0));
        }
        let mut buf = tokio::io::ReadBuf::new(&mut bufs[0]);
        match std::task::ready!(self.sock.lock().unwrap().poll_recv_from(cx, &mut buf)) {
            Ok(addr) => {
                let len = buf.filled().len();
                meta[0] = quinn::udp::RecvMeta {
                    addr,
                    len,
                    stride: len,
                    ecn: None,
                    dst_ip: None,
                };
                std::task::Poll::Ready(Ok(1))
            }
            Err(e) => std::task::Poll::Ready(Err(e)),
        }
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.sock.lock().unwrap().local_addr()
    }
}

#[derive(Debug)]
struct PollWritable {
    sock: Arc<EndToEndSocket>,
}

impl UdpPoller for PollWritable {
    fn poll_writable(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.sock.sock.lock().unwrap().poll_send_ready(cx)
    }
}

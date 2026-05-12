use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

#[cfg(test)]
pub use dens::UdpSocket;
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
            .try_send_to_with_ecn(&transmit.contents, transmit.destination, transmit.ecn)
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
        let mut count = 0;
        for i in 0..bufs.len().min(meta.len()) {
            let mut buf = tokio::io::ReadBuf::new(&mut bufs[i]);
            let sock_lock = self.sock.lock().unwrap();
            match sock_lock.poll_recv_from_with_ecn(cx, &mut buf) {
                std::task::Poll::Ready(Ok((addr, ecn))) => {
                    let len = buf.filled().len();
                    meta[i] = quinn::udp::RecvMeta {
                        addr,
                        len,
                        stride: len,
                        ecn: ecn,
                        dst_ip: Some(sock_lock.local_addr().unwrap().ip()),
                    };
                    count += 1;
                }
                std::task::Poll::Ready(Err(e)) => {
                    if count > 0 {
                        return std::task::Poll::Ready(Ok(count));
                    }
                    return std::task::Poll::Ready(Err(e));
                }
                std::task::Poll::Pending => {
                    if count > 0 {
                        return std::task::Poll::Ready(Ok(count));
                    }
                    return std::task::Poll::Pending;
                }
            }
        }
        std::task::Poll::Ready(Ok(count))
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

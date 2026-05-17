#![allow(clippy::await_holding_refcell_ref)]
#![allow(clippy::unused_async, reason = "it's async in the cargo version")]

use bytes::{BufMut, Bytes};
use tokio::{
    io::{Interest, ReadBuf, Ready},
    sync::mpsc,
};
use tracing::trace;

use std::{
    cell::RefCell,
    fmt,
    io::{self, ErrorKind},
    net::{self, Ipv4Addr, Ipv6Addr, SocketAddr},
    ops::DerefMut,
    task::{Context, Poll},
};

use super::addr::{ToSocketAddrs, to_socket_addrs};
use crate::{OsMock, sim::Sim};

/// A UDP socket.
///
/// UDP is "connectionless", unlike TCP. Meaning, regardless of what address you've bound to, a `UdpSocket`
/// is free to communicate with many different remotes. In tokio there are basically two main ways to use `UdpSocket`:
///
/// * one to many: [`bind`](`UdpSocket::bind`) and use [`send_to`](`UdpSocket::send_to`)
///   and [`recv_from`](`UdpSocket::recv_from`) to communicate with many different addresses
/// * one to one: [`connect`](`UdpSocket::connect`) and associate with a single address, using [`send`](`UdpSocket::send`)
///   and [`recv`](`UdpSocket::recv`) to communicate only with that remote address
///
/// This type does not provide a `split` method, because this functionality
/// can be achieved by instead wrapping the socket in an [`Arc`]. Note that
/// you do not need a `Mutex` to share the `UdpSocket` — an `Arc<UdpSocket>`
/// is enough. This is because all of the methods take `&self` instead of
/// `&mut self`. Once you have wrapped it in an `Arc`, you can call
/// `.clone()` on the `Arc<UdpSocket>` to get multiple shared handles to the
/// same socket. An example of such usage can be found further down.
///
/// [`Arc`]: std::sync::Arc
///
/// # Streams
///
/// If you need to listen over UDP and produce a [`Stream`], you can look
/// at [`UdpFramed`].
///
/// [`UdpFramed`]: https://docs.rs/tokio-util/latest/tokio_util/udp/struct.UdpFramed.html
/// [`Stream`]: https://docs.rs/futures/0.3/futures/stream/trait.Stream.html
///
/// # Example: one to many (bind)
///
/// Using `bind` we can create a simple echo server that sends and recv's with many different clients:
/// ```no_run
/// use dens::os_mock::net::UdpSocket;
/// use std::io;
///
/// #[tokio::main]
/// async fn main() -> io::Result<()> {
///     let sock = UdpSocket::bind("0.0.0.0:8080").await?;
///     let mut buf = [0; 1024];
///     loop {
///         let (len, addr) = sock.recv_from(&mut buf).await?;
///         println!("{:?} bytes received from {:?}", len, addr);
///
///         let len = sock.send_to(&buf[..len], addr).await?;
///         println!("{:?} bytes sent", len);
///     }
/// }
/// ```
///
/// # Example: one to one (connect)
///
/// Or using `connect` we can echo with a single remote address using `send` and `recv`:
/// ```no_run
/// use dens::os_mock::net::UdpSocket;
/// use std::io;
///
/// #[tokio::main]
/// async fn main() -> io::Result<()> {
///     let sock = UdpSocket::bind("0.0.0.0:8080").await?;
///
///     let remote_addr = "127.0.0.1:59611";
///     sock.connect(remote_addr).await?;
///     let mut buf = [0; 1024];
///     loop {
///         let len = sock.recv(&mut buf).await?;
///         println!("{:?} bytes received from {:?}", len, remote_addr);
///
///         let len = sock.send(&buf[..len]).await?;
///         println!("{:?} bytes sent", len);
///     }
/// }
/// ```
///
/// # Example: Splitting with `Arc`
///
/// Because `send_to` and `recv_from` take `&self`. It's perfectly alright
/// to use an `Arc<UdpSocket>` and share the references to multiple tasks.
/// Here is a similar "echo" example that supports concurrent
/// sending/receiving:
///
/// ```no_run
/// use tokio::{net::UdpSocket, sync::mpsc};
/// use std::{io, net::SocketAddr, sync::Arc};
///
/// #[tokio::main]
/// async fn main() -> io::Result<()> {
///     let sock = UdpSocket::bind("0.0.0.0:8080".parse::<SocketAddr>().unwrap()).await?;
///     let r = Arc::new(sock);
///     let s = r.clone();
///     let (tx, mut rx) = mpsc::channel::<(Vec<u8>, SocketAddr)>(1_000);
///
///     tokio::spawn(async move {
///         while let Some((bytes, addr)) = rx.recv().await {
///             let len = s.send_to(&bytes, &addr).await.unwrap();
///             println!("{:?} bytes sent", len);
///         }
///     });
///
///     let mut buf = [0; 1024];
///     loop {
///         let (len, addr) = r.recv_from(&mut buf).await?;
///         println!("{:?} bytes received from {:?}", len, addr);
///         tx.send((buf[..len].to_vec(), addr)).await.unwrap();
///     }
/// }
/// ```
pub struct UdpSocket {
    send: mpsc::Sender<crate::net::udp::Packet>,
    recv: RefCell<mpsc::Receiver<crate::net::udp::Packet>>,
    local_addr: RefCell<SocketAddr>,
    // most recent peer
    peer_addr: RefCell<Option<SocketAddr>>,

    peeked: RefCell<Option<Peeked>>,
}

struct Peeked {
    packet: crate::net::udp::Packet,
    bytes: Bytes,
}

impl From<crate::net::udp::Packet> for Peeked {
    fn from(packet: crate::net::udp::Packet) -> Self {
        let bytes = packet.body();
        Self { packet, bytes }
    }
}

impl UdpSocket {
    /// This function will create a new UDP socket and attempt to bind it to
    /// the `addr` provided.
    ///
    /// Binding with a port number of 0 will request that the OS assigns a port
    /// to this listener. The port allocated can be queried via the `local_addr`
    /// method.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    /// #   if cfg!(miri) { return Ok(()); } // No `socket` in miri.
    ///     let sock = UdpSocket::bind("0.0.0.0:8080").await?;
    ///     // use `sock`
    /// #   let _ = sock;
    ///     Ok(())
    /// }
    /// ```
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<UdpSocket> {
        let addrs = to_socket_addrs(addr)?;
        let mut last_err = None;
        for addr in addrs {
            match Self::bind_addr(addr) {
                Ok(socket) => return Ok(socket),
                Err(e) => last_err = Some(e),
            }
        }

        Err(last_err.ok_or(io::ErrorKind::InvalidInput)?)
    }

    fn bind_addr(addr: SocketAddr) -> io::Result<UdpSocket> {
        let curr_os = Sim::get_current_machine::<OsMock>();
        let (send, recv, local_addr) = curr_os.borrow().bind_to_addr(addr)?;

        Ok(Self {
            send,
            recv: recv.into(),
            local_addr: local_addr.into(),
            peer_addr: RefCell::default(),
            peeked: None.into(),
        })
    }

    /// Creates new `UdpSocket` from a previously bound `std::net::UdpSocket`.
    ///
    /// This function is intended to be used to wrap a UDP socket from the
    /// standard library in the Tokio equivalent.
    ///
    /// This can be used in conjunction with `socket2`'s `Socket` interface to
    /// configure a socket before it's handed off, such as setting options like
    /// `reuse_address` or binding to multiple addresses.
    ///
    /// # Notes
    ///
    /// The caller is responsible for ensuring that the socket is in
    /// non-blocking mode. Otherwise all I/O operations on the socket
    /// will block the thread, which will cause unexpected behavior.
    /// Non-blocking mode can be set using [`set_nonblocking`].
    ///
    /// Passing a listener in blocking mode is always erroneous,
    /// and the behavior in that case may change in the future.
    /// For example, it could panic.
    ///
    /// [`set_nonblocking`]: std::net::UdpSocket::set_nonblocking
    ///
    /// # Panics
    ///
    /// This function panics if thread-local runtime is not set.
    ///
    /// The runtime is usually set implicitly when this function is called
    /// from a future driven by a tokio runtime, otherwise runtime can be set
    /// explicitly with [`Runtime::enter`](crate::runtime::Runtime::enter) function.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// # use std::{io, net::SocketAddr};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> io::Result<()> {
    /// let addr = "0.0.0.0:8080".parse::<SocketAddr>().unwrap();
    /// let std_sock = std::net::UdpSocket::bind(addr)?;
    /// std_sock.set_nonblocking(true)?;
    /// let sock = UdpSocket::from_std(std_sock)?;
    /// // use `sock`
    /// # Ok(())
    /// # }
    /// ```
    #[track_caller]
    pub fn from_std(_socket: net::UdpSocket) -> io::Result<UdpSocket> {
        unimplemented!()
    }

    /// Turns a [`tokio::net::UdpSocket`] into a [`std::net::UdpSocket`].
    ///
    /// The returned [`std::net::UdpSocket`] will have nonblocking mode set as
    /// `true`.  Use [`set_nonblocking`] to change the blocking mode if needed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::error::Error;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let tokio_socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    ///     let std_socket = tokio_socket.into_std()?;
    ///     std_socket.set_nonblocking(false)?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// [`tokio::net::UdpSocket`]: UdpSocket
    /// [`std::net::UdpSocket`]: std::net::UdpSocket
    /// [`set_nonblocking`]: fn@std::net::UdpSocket::set_nonblocking
    pub fn into_std(self) -> io::Result<std::net::UdpSocket> {
        unimplemented!()
    }

    /// Returns the local address that this socket is bound to.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dens::os_mock::net::UdpSocket;
    /// # use std::{io, net::SocketAddr};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> io::Result<()> {
    /// let addr = "0.0.0.0:8080".parse::<SocketAddr>().unwrap();
    /// let sock = UdpSocket::bind(addr).await?;
    /// // the address the socket is bound to
    /// let local_addr = sock.local_addr()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(*self.local_addr.borrow())
    }

    /// Returns the socket address of the remote peer this socket was connected to.
    ///
    /// # Example
    ///
    /// ```
    /// use dens::os_mock::net::UdpSocket;
    /// # use dens::MachineIntoRef as _;
    /// # use dens::sim::Sim;
    /// # use dens::{Host, OsMock, Machine};
    /// # use std::time::Duration;
    /// # use std::{io, net::SocketAddr};
    /// # fn main() -> io::Result<()> {
    /// # let sim = Sim::new();
    /// # sim.enter_runtime(|| {
    /// # let shim = OsMock::new(|| async {
    /// let addr = "0.0.0.0:8080".parse::<SocketAddr>().unwrap();
    /// let peer = "127.0.0.1:11100".parse::<SocketAddr>().unwrap();
    /// let sock = UdpSocket::bind(addr).await?;
    /// sock.connect(peer).await?;
    /// assert_eq!(peer, sock.peer_addr()?);
    /// Ok(())
    /// # }).into_ref();
    /// # let machines = [shim];
    /// # Sim::run_until_idle(|| machines.iter()).unwrap();
    /// #
    /// # });
    /// #    Ok(())
    /// # }
    /// ```
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.peer_addr.borrow().ok_or(ErrorKind::NotConnected)?)
    }

    /// Connects the UDP socket setting the default destination for send() and
    /// limiting packets that are read via `recv` from the address specified in
    /// `addr`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// # use std::{io, net::SocketAddr};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> io::Result<()> {
    /// let sock = UdpSocket::bind("0.0.0.0:8080".parse::<SocketAddr>().unwrap()).await?;
    ///
    /// let remote_addr = "127.0.0.1:59600".parse::<SocketAddr>().unwrap();
    /// sock.connect(remote_addr).await?;
    /// let mut buf = [0u8; 32];
    /// // recv from remote_addr
    /// let len = sock.recv(&mut buf).await?;
    /// // send to remote_addr
    /// let _len = sock.send(&buf[..len]).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect<A: ToSocketAddrs>(&self, addr: A) -> io::Result<()> {
        *self.peer_addr.borrow_mut() = to_socket_addrs(addr).unwrap().next();
        Ok(())
    }

    /// Waits for any of the requested ready states.
    ///
    /// This function is usually paired with `try_recv()` or `try_send()`. It
    /// can be used to concurrently `recv` / `send` to the same socket on a single
    /// task without splitting the socket.
    ///
    /// The function may complete without the socket being ready. This is a
    /// false-positive and attempting an operation will return with
    /// `io::ErrorKind::WouldBlock`. The function can also return with an empty
    /// [`Ready`] set, so you should always check the returned value and possibly
    /// wait again if the requested states are not set.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. Once a readiness event occurs, the method
    /// will continue to return immediately until the readiness event is
    /// consumed by an attempt to read or write that fails with `WouldBlock` or
    /// `Poll::Pending`.
    ///
    /// # Examples
    ///
    /// Concurrently receive from and send to the socket on the same task
    /// without splitting.
    ///
    /// ```no_run
    /// use tokio::io::{self, Interest};
    /// use dens::os_mock::net::UdpSocket;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///     socket.connect("127.0.0.1:8081").await?;
    ///
    ///     loop {
    ///         let ready = socket.ready(Interest::READABLE | Interest::WRITABLE).await?;
    ///
    ///         if ready.is_readable() {
    ///             // The buffer is **not** included in the async task and will only exist
    ///             // on the stack.
    ///             let mut data = [0; 1024];
    ///             match socket.try_recv(&mut data[..]) {
    ///                 Ok(n) => {
    ///                     println!("received {:?}", &data[..n]);
    ///                 }
    ///                 // False-positive, continue
    ///                 Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
    ///                 Err(e) => {
    ///                     return Err(e);
    ///                 }
    ///             }
    ///         }
    ///
    ///         if ready.is_writable() {
    ///             // Write some data
    ///             match socket.try_send(b"hello world") {
    ///                 Ok(n) => {
    ///                     println!("sent {} bytes", n);
    ///                 }
    ///                 // False-positive, continue
    ///                 Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
    ///                 Err(e) => {
    ///                     return Err(e);
    ///                 }
    ///             }
    ///         }
    ///     }
    /// }
    /// ```
    pub async fn ready(&self, interest: Interest) -> io::Result<Ready> {
        let mut ready = Ready::EMPTY;

        // In our simulated socket the send channel is always writable unless closed.
        if interest.is_writable() && !self.send.is_closed() {
            ready |= Ready::WRITABLE;
        }

        if interest.is_readable() {
            if self.peeked.borrow().is_some() {
                // Already have a peeked packet buffered — readable immediately.
                ready |= Ready::READABLE;
            } else {
                // Try a non-blocking peek at the recv channel.
                match self.recv.borrow_mut().try_recv() {
                    Ok(packet) => {
                        *self.peeked.borrow_mut() = Some(Peeked::from(packet));
                        ready |= Ready::READABLE;
                    }
                    Err(mpsc::error::TryRecvError::Empty) => {
                        // Nothing available yet; will wait below if needed.
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        return Err(io::Error::from(ErrorKind::BrokenPipe));
                    }
                }
            }
        }

        // If any requested interest is already satisfied, return immediately.
        if !ready.is_empty() {
            return Ok(ready);
        }

        // Nothing is immediately ready.  The only case where we must actually
        // suspend is when READABLE was requested but no data has arrived yet
        // (WRITABLE would already be set above if the channel is open).
        if interest.is_readable() {
            let packet = self
                .recv
                .borrow_mut()
                .recv()
                .await
                .ok_or_else(|| io::Error::from(ErrorKind::BrokenPipe))?;
            *self.peeked.borrow_mut() = Some(Peeked::from(packet));
            ready |= Ready::READABLE;
        }

        Ok(ready)
    }

    /// Waits for the socket to become writable.
    ///
    /// This function is equivalent to `ready(Interest::WRITABLE)` and is
    /// usually paired with `try_send()` or `try_send_to()`.
    ///
    /// The function may complete without the socket being writable. This is a
    /// false-positive and attempting a `try_send()` will return with
    /// `io::ErrorKind::WouldBlock`.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. Once a readiness event occurs, the method
    /// will continue to return immediately until the readiness event is
    /// consumed by an attempt to write that fails with `WouldBlock` or
    /// `Poll::Pending`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     // Bind socket
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///     socket.connect("127.0.0.1:8081").await?;
    ///
    ///     loop {
    ///         // Wait for the socket to be writable
    ///         socket.writable().await?;
    ///
    ///         // Try to send data, this may still fail with `WouldBlock`
    ///         // if the readiness event is a false positive.
    ///         match socket.try_send(b"hello world") {
    ///             Ok(n) => {
    ///                 break;
    ///             }
    ///             Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
    ///                 continue;
    ///             }
    ///             Err(e) => {
    ///                 return Err(e);
    ///             }
    ///         }
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn writable(&self) -> io::Result<()> {
        self.ready(Interest::WRITABLE).await?;
        Ok(())
    }

    /// Polls for write/send readiness.
    ///
    /// If the udp stream is not currently ready for sending, this method will
    /// store a clone of the `Waker` from the provided `Context`. When the udp
    /// stream becomes ready for sending, `Waker::wake` will be called on the
    /// waker.
    ///
    /// Note that on multiple calls to `poll_send_ready` or `poll_send`, only
    /// the `Waker` from the `Context` passed to the most recent call is
    /// scheduled to receive a wakeup. (However, `poll_recv_ready` retains a
    /// second, independent waker.)
    ///
    /// This function is intended for cases where creating and pinning a future
    /// via [`writable`] is not feasible. Where possible, using [`writable`] is
    /// preferred, as this supports polling from multiple tasks at once.
    ///
    /// # Return value
    ///
    /// The function returns:
    ///
    /// * `Poll::Pending` if the udp stream is not ready for writing.
    /// * `Poll::Ready(Ok(()))` if the udp stream is ready for writing.
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Errors
    ///
    /// This function may encounter any standard I/O error except `WouldBlock`.
    ///
    /// [`writable`]: method@Self::writable
    pub fn poll_send_ready(&self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // In our simulated socket the send channel is always immediately writable
        // unless the receiver has been dropped.
        if self.send.is_closed() {
            Poll::Ready(Err(io::Error::from(ErrorKind::BrokenPipe)))
        } else {
            Poll::Ready(Ok(()))
        }
    }

    /// Sends data on the socket to the remote address that the socket is
    /// connected to.
    ///
    /// The [`connect`] method will connect this socket to a remote address.
    /// This method will fail if the socket is not connected.
    ///
    /// [`connect`]: method@Self::connect
    ///
    /// # Return
    ///
    /// On success, the number of bytes sent is returned, otherwise, the
    /// encountered error is returned.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. If `send` is used as the event in a
    /// [`tokio::select!`](crate::select) statement and some other branch
    /// completes first, then it is guaranteed that the message was not sent.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tokio::io;
    /// use dens::os_mock::net::UdpSocket;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     // Bind socket
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///     socket.connect("127.0.0.1:8081").await?;
    ///
    ///     // Send a message
    ///     socket.send(b"hello world").await?;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn send(&self, buf: &[u8]) -> io::Result<usize> {
        // limits buf size to be < 1472 bytes (emulates ethernet)
        // maybe swap out with a config value
        let buf = &buf[..std::cmp::min(buf.len(), 1472)];
        let packet = crate::net::udp::Packet::new(
            *self.local_addr.borrow(),
            self.peer_addr.borrow().ok_or(ErrorKind::NotConnected)?,
            buf,
            None,
        );
        self.send
            .send(packet)
            .await
            .or(Err(ErrorKind::HostUnreachable))?;
        Ok(buf.len())
    }

    /// Attempts to send data on the socket to the remote address to which it
    /// was previously `connect`ed.
    ///
    /// The [`connect`] method will connect this socket to a remote address.
    /// This method will fail if the socket is not connected.
    ///
    /// Note that on multiple calls to a `poll_*` method in the send direction,
    /// only the `Waker` from the `Context` passed to the most recent call will
    /// be scheduled to receive a wakeup.
    ///
    /// # Return value
    ///
    /// The function returns:
    ///
    /// * `Poll::Pending` if the socket is not available to write
    /// * `Poll::Ready(Ok(n))` `n` is the number of bytes sent
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Errors
    ///
    /// This function may encounter any standard I/O error except `WouldBlock`.
    ///
    /// [`connect`]: method@Self::connect
    pub fn poll_send(&self, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match self.try_send(buf) {
            Ok(n) => Poll::Ready(Ok(n)),
            Err(e) if e.kind() == ErrorKind::WouldBlock => Poll::Pending,
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    /// Tries to send data on the socket to the remote address to which it is
    /// connected.
    ///
    /// When the socket buffer is full, `Err(io::ErrorKind::WouldBlock)` is
    /// returned. This function is usually paired with `writable()`.
    ///
    /// # Returns
    ///
    /// If successful, `Ok(n)` is returned, where `n` is the number of bytes
    /// sent. If the socket is not ready to send data,
    /// `Err(ErrorKind::WouldBlock)` is returned.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     // Bind a UDP socket
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///
    ///     // Connect to a peer
    ///     socket.connect("127.0.0.1:8081").await?;
    ///
    ///     loop {
    ///         // Wait for the socket to be writable
    ///         socket.writable().await?;
    ///
    ///         // Try to send data, this may still fail with `WouldBlock`
    ///         // if the readiness event is a false positive.
    ///         match socket.try_send(b"hello world") {
    ///             Ok(n) => {
    ///                 break;
    ///             }
    ///             Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
    ///                 continue;
    ///             }
    ///             Err(e) => {
    ///                 return Err(e);
    ///             }
    ///         }
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn try_send(&self, buf: &[u8]) -> io::Result<usize> {
        // limits buf size to be < 1472 bytes (emulates ethernet)
        let buf = &buf[..std::cmp::min(buf.len(), 1472)];
        let packet = crate::net::udp::Packet::new(
            *self.local_addr.borrow(),
            self.peer_addr
                .borrow()
                .ok_or_else(|| io::Error::from(ErrorKind::NotConnected))?,
            buf,
            None,
        );
        match self.send.try_send(packet) {
            Ok(()) => Ok(buf.len()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(io::Error::from(ErrorKind::WouldBlock)),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err(io::Error::from(ErrorKind::HostUnreachable))
            }
        }
    }

    /// Waits for the socket to become readable.
    ///
    /// This function is equivalent to `ready(Interest::READABLE)` and is usually
    /// paired with `try_recv()`.
    ///
    /// The function may complete without the socket being readable. This is a
    /// false-positive and attempting a `try_recv()` will return with
    /// `io::ErrorKind::WouldBlock`.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. Once a readiness event occurs, the method
    /// will continue to return immediately until the readiness event is
    /// consumed by an attempt to read that fails with `WouldBlock` or
    /// `Poll::Pending`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     // Connect to a peer
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///     socket.connect("127.0.0.1:8081").await?;
    ///
    ///     loop {
    ///         // Wait for the socket to be readable
    ///         socket.readable().await?;
    ///
    ///         // The buffer is **not** included in the async task and will
    ///         // only exist on the stack.
    ///         let mut buf = [0; 1024];
    ///
    ///         // Try to recv data, this may still fail with `WouldBlock`
    ///         // if the readiness event is a false positive.
    ///         match socket.try_recv(&mut buf) {
    ///             Ok(n) => {
    ///                 println!("GOT {:?}", &buf[..n]);
    ///                 break;
    ///             }
    ///             Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
    ///                 continue;
    ///             }
    ///             Err(e) => {
    ///                 return Err(e);
    ///             }
    ///         }
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn readable(&self) -> io::Result<()> {
        self.ready(Interest::READABLE).await?;
        Ok(())
    }

    /// Polls for read/receive readiness.
    ///
    /// If the udp stream is not currently ready for receiving, this method will
    /// store a clone of the `Waker` from the provided `Context`. When the udp
    /// socket becomes ready for reading, `Waker::wake` will be called on the
    /// waker.
    ///
    /// Note that on multiple calls to `poll_recv_ready`, `poll_recv` or
    /// `poll_peek`, only the `Waker` from the `Context` passed to the most
    /// recent call is scheduled to receive a wakeup. (However,
    /// `poll_send_ready` retains a second, independent waker.)
    ///
    /// This function is intended for cases where creating and pinning a future
    /// via [`readable`] is not feasible. Where possible, using [`readable`] is
    /// preferred, as this supports polling from multiple tasks at once.
    ///
    /// # Return value
    ///
    /// The function returns:
    ///
    /// * `Poll::Pending` if the udp stream is not ready for reading.
    /// * `Poll::Ready(Ok(()))` if the udp stream is ready for reading.
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Errors
    ///
    /// This function may encounter any standard I/O error except `WouldBlock`.
    ///
    /// [`readable`]: method@Self::readable
    pub fn poll_recv_ready(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Fast path: a packet is already buffered from a previous peek/recv.
        if self.peeked.borrow().is_some() {
            return Poll::Ready(Ok(()));
        }

        // Delegate to the underlying mpsc receiver so that the waker stored in
        // `cx` is registered and will be woken when a packet arrives.
        match self.recv.borrow_mut().poll_recv(cx) {
            Poll::Ready(Some(packet)) => {
                *self.peeked.borrow_mut() = Some(Peeked::from(packet));
                Poll::Ready(Ok(()))
            }
            Poll::Ready(None) => Poll::Ready(Err(io::Error::from(ErrorKind::BrokenPipe))),
            Poll::Pending => Poll::Pending,
        }
    }

    /// Receives a single datagram message on the socket from the remote address
    /// to which it is connected. On success, returns the number of bytes read.
    ///
    /// The function must be called with valid byte array `buf` of sufficient
    /// size to hold the message bytes. If a message is too long to fit in the
    /// supplied buffer, excess bytes may be discarded.
    ///
    /// The [`connect`] method will connect this socket to a remote address.
    /// This method will fail if the socket is not connected.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. If `recv` is used as the event in a
    /// [`tokio::select!`](crate::select) statement and some other branch
    /// completes first, it is guaranteed that no messages were received on this
    /// socket.
    ///
    /// [`connect`]: method@Self::connect
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     // Bind socket
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///     socket.connect("127.0.0.1:8081").await?;
    ///
    ///     let mut buf = vec![0; 10];
    ///     let n = socket.recv(&mut buf).await?;
    ///
    ///     println!("received {} bytes {:?}", n, &buf[..n]);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.try_recv(buf) {
                Ok(n) => return Ok(n),
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    std::future::poll_fn(|cx| self.poll_recv_ready(cx)).await?;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Attempts to receive a single datagram message on the socket from the remote
    /// address to which it is `connect`ed.
    ///
    /// The [`connect`] method will connect this socket to a remote address. This method
    /// resolves to an error if the socket is not connected.
    ///
    /// Note that on multiple calls to a `poll_*` method in the `recv` direction, only the
    /// `Waker` from the `Context` passed to the most recent call will be scheduled to
    /// receive a wakeup.
    ///
    /// # Return value
    ///
    /// The function returns:
    ///
    /// * `Poll::Pending` if the socket is not ready to read
    /// * `Poll::Ready(Ok(()))` reads data `ReadBuf` if the socket is ready
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Errors
    ///
    /// This function may encounter any standard I/O error except `WouldBlock`.
    ///
    /// [`connect`]: method@Self::connect
    pub fn poll_recv(&self, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let mut borrow = self.peeked.borrow_mut();
        std::task::ready!(self.poll_fill_peeked(cx, &mut borrow))?;
        let chunk = Self::read_peeked(&mut borrow, buf.remaining());
        buf.put_slice(&chunk);
        Poll::Ready(Ok(()))
    }

    /// Tries to receive a single datagram message on the socket from the remote
    /// address to which it is connected. On success, returns the number of
    /// bytes read.
    ///
    /// This method must be called with valid byte array `buf` of sufficient size
    /// to hold the message bytes. If a message is too long to fit in the
    /// supplied buffer, excess bytes may be discarded.
    ///
    /// When there is no pending data, `Err(io::ErrorKind::WouldBlock)` is
    /// returned. This function is usually paired with `readable()`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     // Connect to a peer
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///     socket.connect("127.0.0.1:8081").await?;
    ///
    ///     loop {
    ///         // Wait for the socket to be readable
    ///         socket.readable().await?;
    ///
    ///         // The buffer is **not** included in the async task and will
    ///         // only exist on the stack.
    ///         let mut buf = [0; 1024];
    ///
    ///         // Try to recv data, this may still fail with `WouldBlock`
    ///         // if the readiness event is a false positive.
    ///         match socket.try_recv(&mut buf) {
    ///             Ok(n) => {
    ///                 println!("GOT {:?}", &buf[..n]);
    ///                 break;
    ///             }
    ///             Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
    ///                 continue;
    ///             }
    ///             Err(e) => {
    ///                 return Err(e);
    ///             }
    ///         }
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn try_recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut borrow = self.peeked.borrow_mut();
        self.try_fill_peeked(&mut borrow)?;
        let chunk = Self::read_peeked(&mut borrow, buf.len());
        buf[..chunk.len()].copy_from_slice(&chunk);
        Ok(chunk.len())
    }

    /// Tries to receive data from the stream into the provided buffer, advancing the
    /// buffer's internal cursor, returning how many bytes were read.
    ///
    /// This method must be called with valid byte array `buf` of sufficient size
    /// to hold the message bytes. If a message is too long to fit in the
    /// supplied buffer, excess bytes may be discarded.
    ///
    /// This method can be used even if `buf` is uninitialized.
    ///
    /// When there is no pending data, `Err(io::ErrorKind::WouldBlock)` is
    /// returned. This function is usually paired with `readable()`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     // Connect to a peer
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///     socket.connect("127.0.0.1:8081").await?;
    ///
    ///     loop {
    ///         // Wait for the socket to be readable
    ///         socket.readable().await?;
    ///
    ///         let mut buf = Vec::with_capacity(1024);
    ///
    ///         // Try to recv data, this may still fail with `WouldBlock`
    ///         // if the readiness event is a false positive.
    ///         match socket.try_recv_buf(&mut buf) {
    ///             Ok(n) => {
    ///                 println!("GOT {:?}", &buf[..n]);
    ///                 break;
    ///             }
    ///             Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
    ///                 continue;
    ///             }
    ///             Err(e) => {
    ///                 return Err(e);
    ///             }
    ///         }
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn try_recv_buf<B: BufMut>(&self, buf: &mut B) -> io::Result<usize> {
        let mut borrow = self.peeked.borrow_mut();
        self.try_fill_peeked(&mut borrow)?;

        let chunk = Self::read_peeked(&mut borrow, buf.remaining_mut());
        let n = chunk.len();
        buf.put_slice(&chunk);

        Ok(n)
    }

    /// Receives a single datagram message on the socket from the remote address
    /// to which it is connected, advancing the buffer's internal cursor,
    /// returning how many bytes were read.
    ///
    /// This method must be called with valid byte array `buf` of sufficient size
    /// to hold the message bytes. If a message is too long to fit in the
    /// supplied buffer, excess bytes may be discarded.
    ///
    /// This method can be used even if `buf` is uninitialized.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     // Connect to a peer
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///     socket.connect("127.0.0.1:8081").await?;
    ///
    ///     let mut buf = Vec::with_capacity(512);
    ///     let len = socket.recv_buf(&mut buf).await?;
    ///
    ///     println!("received {} bytes {:?}", len, &buf[..len]);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn recv_buf<B: BufMut>(&self, buf: &mut B) -> io::Result<usize> {
        let mut borrow = self.peeked.borrow_mut();
        self.fill_peeked(&mut borrow).await?;

        let chunk = Self::read_peeked(&mut borrow, buf.remaining_mut());
        let n = chunk.len();
        buf.put_slice(&chunk);

        Ok(n)
    }

    /// Tries to receive a single datagram message on the socket. On success,
    /// returns the number of bytes read and the origin.
    ///
    /// This method must be called with valid byte array `buf` of sufficient size
    /// to hold the message bytes. If a message is too long to fit in the
    /// supplied buffer, excess bytes may be discarded.
    ///
    /// This method can be used even if `buf` is uninitialized.
    ///
    /// When there is no pending data, `Err(io::ErrorKind::WouldBlock)` is
    /// returned. This function is usually paired with `readable()`.
    ///
    /// # Notes
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     // Connect to a peer
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///
    ///     loop {
    ///         // Wait for the socket to be readable
    ///         socket.readable().await?;
    ///
    ///         let mut buf = Vec::with_capacity(1024);
    ///
    ///         // Try to recv data, this may still fail with `WouldBlock`
    ///         // if the readiness event is a false positive.
    ///         match socket.try_recv_buf_from(&mut buf) {
    ///             Ok((n, _addr)) => {
    ///                 println!("GOT {:?}", &buf[..n]);
    ///                 break;
    ///             }
    ///             Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
    ///                 continue;
    ///             }
    ///             Err(e) => {
    ///                 return Err(e);
    ///             }
    ///         }
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn try_recv_buf_from<B: BufMut>(&self, buf: &mut B) -> io::Result<(usize, SocketAddr)> {
        let mut borrow = self.peeked.borrow_mut();
        self.try_fill_peeked(&mut borrow)?;
        let from = borrow.as_ref().unwrap().packet.header().get_src_addr();
        let chunk = Self::read_peeked(&mut borrow, buf.remaining_mut());
        let n = chunk.len();
        buf.put_slice(&chunk);
        Ok((n, from))
    }

    /// Receives a single datagram message on the socket, advancing the
    /// buffer's internal cursor, returning how many bytes were read and the origin.
    ///
    /// This method must be called with valid byte array `buf` of sufficient size
    /// to hold the message bytes. If a message is too long to fit in the
    /// supplied buffer, excess bytes may be discarded.
    ///
    /// This method can be used even if `buf` is uninitialized.
    ///
    /// # Notes
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     // Connect to a peer
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///     socket.connect("127.0.0.1:8081").await?;
    ///
    ///     let mut buf = Vec::with_capacity(512);
    ///     let (len, addr) = socket.recv_buf_from(&mut buf).await?;
    ///
    ///     println!("received {:?} bytes from {:?}", len, addr);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn recv_buf_from<B: BufMut>(&self, buf: &mut B) -> io::Result<(usize, SocketAddr)> {
        let mut borrow = self.peeked.borrow_mut();
        self.fill_peeked(&mut borrow).await?;
        let from = borrow.as_ref().unwrap().packet.header().get_src_addr();
        let chunk = Self::read_peeked(&mut borrow, buf.remaining_mut());
        let n = chunk.len();
        buf.put_slice(&chunk);
        Ok((n, from))
    }

    /// Sends data on the socket to the given address. On success, returns the
    /// number of bytes written.
    ///
    /// Address type can be any implementor of [`ToSocketAddrs`] trait. See its
    /// documentation for concrete examples.
    ///
    /// It is possible for `addr` to yield multiple addresses, but `send_to`
    /// will only send data to the first address yielded by `addr`.
    ///
    /// This will return an error when the IP version of the local socket does
    /// not match that returned from [`ToSocketAddrs`].
    ///
    /// [`ToSocketAddrs`]: crate::net::ToSocketAddrs
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. If `send_to` is used as the event in a
    /// [`tokio::select!`](crate::select) statement and some other branch
    /// completes first, then it is guaranteed that the message was not sent.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///     let len = socket.send_to(b"hello world", "127.0.0.1:8081").await?;
    ///
    ///     println!("Sent {} bytes", len);
    ///
    ///     Ok(())
    /// }
    /// ```
    #[allow(clippy::unused_async, reason = "it's async in the cargo version")]
    pub async fn send_to<A: ToSocketAddrs>(&self, buf: &[u8], addr: A) -> io::Result<usize> {
        let target = to_socket_addrs(addr)?
            .next()
            .ok_or_else(|| io::Error::from(ErrorKind::InvalidInput))?;
        // limits buf size to be < 1472 bytes (emulates ethernet)
        let buf = &buf[..std::cmp::min(buf.len(), 1472)];
        let packet = crate::net::udp::Packet::new(*self.local_addr.borrow(), target, buf, None);
        self.send
            .try_send(packet)
            .or(Err(io::Error::from(ErrorKind::HostUnreachable)))?;
        Ok(buf.len())
    }

    /// Attempts to send data on the socket to a given address.
    ///
    /// Note that on multiple calls to a `poll_*` method in the send direction, only the
    /// `Waker` from the `Context` passed to the most recent call will be scheduled to
    /// receive a wakeup.
    ///
    /// # Return value
    ///
    /// The function returns:
    ///
    /// * `Poll::Pending` if the socket is not ready to write
    /// * `Poll::Ready(Ok(n))` `n` is the number of bytes sent.
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Errors
    ///
    /// This function may encounter any standard I/O error except `WouldBlock`.
    pub fn poll_send_to(
        &self,
        _cx: &mut Context<'_>,
        buf: &[u8],
        target: SocketAddr,
    ) -> Poll<io::Result<usize>> {
        match self.try_send_to(buf, target) {
            Ok(n) => Poll::Ready(Ok(n)),
            Err(e) if e.kind() == ErrorKind::WouldBlock => Poll::Pending,
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    /// Tries to send data on the socket to the given address, but if the send is
    /// blocked this will return right away.
    ///
    /// This function is usually paired with `writable()`.
    ///
    /// # Returns
    ///
    /// If successful, returns the number of bytes sent
    ///
    /// Users should ensure that when the remote cannot receive, the
    /// [`ErrorKind::WouldBlock`] is properly handled. An error can also occur
    /// if the IP version of the socket does not match that of `target`.
    ///
    /// [`ErrorKind::WouldBlock`]: std::io::ErrorKind::WouldBlock
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::error::Error;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///
    ///     let dst = "127.0.0.1:8081".parse()?;
    ///
    ///     loop {
    ///         socket.writable().await?;
    ///
    ///         match socket.try_send_to(&b"hello world"[..], dst) {
    ///             Ok(sent) => {
    ///                 println!("sent {} bytes", sent);
    ///                 break;
    ///             }
    ///             Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
    ///                 // Writable false positive.
    ///                 continue;
    ///             }
    ///             Err(e) => return Err(e.into()),
    ///         }
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn try_send_to(&self, buf: &[u8], target: SocketAddr) -> io::Result<usize> {
        // limits buf size to be < 1472 bytes (emulates ethernet)
        let buf = &buf[..std::cmp::min(buf.len(), 1472)];
        let packet = crate::net::udp::Packet::new(*self.local_addr.borrow(), target, buf, None);
        match self.send.try_send(packet) {
            Ok(()) => Ok(buf.len()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(io::Error::from(ErrorKind::WouldBlock)),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err(io::Error::from(ErrorKind::HostUnreachable))
            }
        }
    }

    pub fn try_send_to_with_ecn(
        &self,
        buf: &[u8],
        target: SocketAddr,
        ecn: Option<quinn_udp::EcnCodepoint>,
    ) -> io::Result<usize> {
        // limits buf size to be < 1472 bytes (emulates ethernet)
        let buf = &buf[..std::cmp::min(buf.len(), 1472)];
        let packet = crate::net::udp::Packet::new(*self.local_addr.borrow(), target, buf, ecn);
        trace!("sending packet");
        match self.send.try_send(packet) {
            Ok(()) => Ok(buf.len()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(io::Error::from(ErrorKind::WouldBlock)),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err(io::Error::from(ErrorKind::HostUnreachable))
            }
        }
    }

    /// Receives a single datagram message on the socket. On success, returns
    /// the number of bytes read and the origin.
    ///
    /// The function must be called with valid byte array `buf` of sufficient
    /// size to hold the message bytes. If a message is too long to fit in the
    /// supplied buffer, excess bytes may be discarded.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. If `recv_from` is used as the event in a
    /// [`tokio::select!`](crate::select) statement and some other branch
    /// completes first, it is guaranteed that no messages were received on this
    /// socket.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///
    ///     let mut buf = vec![0u8; 32];
    ///     let (len, addr) = socket.recv_from(&mut buf).await?;
    ///
    ///     println!("received {:?} bytes from {:?}", len, addr);
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Notes
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    pub async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let mut borrow = self.peeked.borrow_mut();
        self.fill_peeked(&mut borrow).await?;
        let from = borrow.as_ref().unwrap().packet.header().get_src_addr();
        let chunk = Self::read_peeked(&mut borrow, buf.len());
        buf[..chunk.len()].copy_from_slice(&chunk);
        Ok((chunk.len(), from))
    }

    async fn fill_peeked(
        &self,
        peeked: &mut impl DerefMut<Target = Option<Peeked>>,
    ) -> io::Result<()> {
        if peeked.is_none() {
            let packet = self
                .recv
                .borrow_mut()
                .recv()
                .await
                .ok_or(ErrorKind::BrokenPipe)?;
            peeked.replace(Peeked::from(packet));
        }
        Ok(())
    }

    fn try_fill_peeked(
        &self,
        peeked: &mut impl DerefMut<Target = Option<Peeked>>,
    ) -> io::Result<()> {
        if peeked.is_none() {
            match self.recv.borrow_mut().try_recv() {
                Ok(packet) => {
                    peeked.replace(Peeked::from(packet));
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    return Err(io::Error::from(ErrorKind::WouldBlock));
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return Err(io::Error::from(ErrorKind::BrokenPipe));
                }
            }
        }
        Ok(())
    }

    fn poll_fill_peeked(
        &self,
        cx: &mut Context<'_>,
        peeked: &mut impl DerefMut<Target = Option<Peeked>>,
    ) -> Poll<io::Result<()>> {
        if peeked.is_none() {
            match self.recv.borrow_mut().poll_recv(cx) {
                Poll::Ready(Some(packet)) => {
                    peeked.replace(Peeked::from(packet));
                }
                Poll::Ready(None) => {
                    return Poll::Ready(Err(io::Error::from(ErrorKind::BrokenPipe)));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(()))
    }

    /// Extracts up to `max` bytes from the buffered datagram and discards
    /// the rest, consuming the datagram entirely (standard UDP semantics).
    fn read_peeked(borrow: &mut Option<Peeked>, max: usize) -> Bytes {
        let peeked = borrow.as_mut().unwrap();
        let n = peeked.bytes.len().min(max);
        let chunk = peeked.bytes.slice(..n);
        *borrow = None;
        chunk
    }

    /// Attempts to receive a single datagram on the socket.
    ///
    /// Note that on multiple calls to a `poll_*` method in the `recv` direction, only the
    /// `Waker` from the `Context` passed to the most recent call will be scheduled to
    /// receive a wakeup.
    ///
    /// # Return value
    ///
    /// The function returns:
    ///
    /// * `Poll::Pending` if the socket is not ready to read
    /// * `Poll::Ready(Ok(addr))` reads data from `addr` into `ReadBuf` if the socket is ready
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Errors
    ///
    /// This function may encounter any standard I/O error except `WouldBlock`.
    ///
    /// # Notes
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    pub fn poll_recv_from(
        &self,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<SocketAddr>> {
        let mut borrow = self.peeked.borrow_mut();
        std::task::ready!(self.poll_fill_peeked(cx, &mut borrow))?;
        let from = borrow.as_ref().unwrap().packet.header().get_src_addr();
        let chunk = Self::read_peeked(&mut borrow, buf.remaining());
        buf.put_slice(&chunk);
        Poll::Ready(Ok(from))
    }

    pub fn poll_recv_from_with_ecn(
        &self,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<(SocketAddr, Option<quinn_udp::EcnCodepoint>)>> {
        let mut borrow = self.peeked.borrow_mut();
        std::task::ready!(self.poll_fill_peeked(cx, &mut borrow))?;
        let header = borrow.as_ref().unwrap().packet.header();
        let from = header.get_src_addr();
        let to = header.get_dst_addr();
        *self.local_addr.borrow_mut() = to;
        let ecn = header.ip_header().ecn();
        let chunk = Self::read_peeked(&mut borrow, buf.remaining());
        buf.put_slice(&chunk);
        Poll::Ready(Ok((from, ecn)))
    }

    /// Tries to receive a single datagram message on the socket. On success,
    /// returns the number of bytes read and the origin.
    ///
    /// This method must be called with valid byte array `buf` of sufficient size
    /// to hold the message bytes. If a message is too long to fit in the
    /// supplied buffer, excess bytes may be discarded.
    ///
    /// When there is no pending data, `Err(io::ErrorKind::WouldBlock)` is
    /// returned. This function is usually paired with `readable()`.
    ///
    /// # Notes
    ///
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     // Connect to a peer
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///
    ///     loop {
    ///         // Wait for the socket to be readable
    ///         socket.readable().await?;
    ///
    ///         // The buffer is **not** included in the async task and will
    ///         // only exist on the stack.
    ///         let mut buf = [0; 1024];
    ///
    ///         // Try to recv data, this may still fail with `WouldBlock`
    ///         // if the readiness event is a false positive.
    ///         match socket.try_recv_from(&mut buf) {
    ///             Ok((n, _addr)) => {
    ///                 println!("GOT {:?}", &buf[..n]);
    ///                 break;
    ///             }
    ///             Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
    ///                 continue;
    ///             }
    ///             Err(e) => {
    ///                 return Err(e);
    ///             }
    ///         }
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn try_recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let mut borrow = self.peeked.borrow_mut();
        self.try_fill_peeked(&mut borrow)?;
        let from = borrow.as_ref().unwrap().packet.header().get_src_addr();
        let chunk = Self::read_peeked(&mut borrow, buf.len());
        buf[..chunk.len()].copy_from_slice(&chunk);
        Ok((chunk.len(), from))
    }

    /// Tries to read or write from the socket using a user-provided IO operation.
    ///
    /// If the socket is ready, the provided closure is called. The closure
    /// should attempt to perform IO operation on the socket by manually
    /// calling the appropriate syscall. If the operation fails because the
    /// socket is not actually ready, then the closure should return a
    /// `WouldBlock` error and the readiness flag is cleared. The return value
    /// of the closure is then returned by `try_io`.
    ///
    /// If the socket is not ready, then the closure is not called
    /// and a `WouldBlock` error is returned.
    ///
    /// The closure should only return a `WouldBlock` error if it has performed
    /// an IO operation on the socket that failed due to the socket not being
    /// ready. Returning a `WouldBlock` error in any other situation will
    /// incorrectly clear the readiness flag, which can cause the socket to
    /// behave incorrectly.
    ///
    /// The closure should not perform the IO operation using any of the methods
    /// defined on the Tokio `UdpSocket` type, as this will mess with the
    /// readiness flag and can cause the socket to behave incorrectly.
    ///
    /// This method is not intended to be used with combined interests.
    /// The closure should perform only one type of IO operation, so it should not
    /// require more than one ready state. This method may panic or sleep forever
    /// if it is called with a combined interest.
    ///
    /// Usually, [`readable()`], [`writable()`] or [`ready()`] is used with this function.
    ///
    /// [`readable()`]: UdpSocket::readable()
    /// [`writable()`]: UdpSocket::writable()
    /// [`ready()`]: UdpSocket::ready()
    pub fn try_io<R>(
        &self,
        interest: Interest,
        f: impl FnOnce() -> io::Result<R>,
    ) -> io::Result<R> {
        let mut ready = false;

        if interest.is_writable() && !self.send.is_closed() {
            ready = true;
        }

        if interest.is_readable() {
            if self.peeked.borrow().is_some() {
                ready = true;
            } else {
                match self.recv.borrow_mut().try_recv() {
                    Ok(packet) => {
                        *self.peeked.borrow_mut() = Some(Peeked::from(packet));
                        ready = true;
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        return Err(io::Error::from(io::ErrorKind::BrokenPipe));
                    }
                    Err(mpsc::error::TryRecvError::Empty) => {}
                }
            }
        }

        if !ready {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }

        f()
    }

    /// Reads or writes from the socket using a user-provided IO operation.
    ///
    /// The readiness of the socket is awaited and when the socket is ready,
    /// the provided closure is called. The closure should attempt to perform
    /// IO operation on the socket by manually calling the appropriate syscall.
    /// If the operation fails because the socket is not actually ready,
    /// then the closure should return a `WouldBlock` error. In such case the
    /// readiness flag is cleared and the socket readiness is awaited again.
    /// This loop is repeated until the closure returns an `Ok` or an error
    /// other than `WouldBlock`.
    ///
    /// The closure should only return a `WouldBlock` error if it has performed
    /// an IO operation on the socket that failed due to the socket not being
    /// ready. Returning a `WouldBlock` error in any other situation will
    /// incorrectly clear the readiness flag, which can cause the socket to
    /// behave incorrectly.
    ///
    /// The closure should not perform the IO operation using any of the methods
    /// defined on the Tokio `UdpSocket` type, as this will mess with the
    /// readiness flag and can cause the socket to behave incorrectly.
    ///
    /// This method is not intended to be used with combined interests.
    /// The closure should perform only one type of IO operation, so it should not
    /// require more than one ready state. This method may panic or sleep forever
    /// if it is called with a combined interest.
    pub async fn async_io<R>(
        &self,
        interest: Interest,
        mut f: impl FnMut() -> io::Result<R>,
    ) -> io::Result<R> {
        loop {
            self.ready(interest).await?;
            match f() {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                res => return res,
            }
        }
    }

    /// Receives a single datagram from the connected address without removing it from the queue.
    /// On success, returns the number of bytes read from whence the data came.
    ///
    /// # Notes
    ///
    /// On Windows, if the data is larger than the buffer specified, the buffer
    /// is filled with the first part of the data, and peek returns the error
    /// `WSAEMSGSIZE(10040)`. The excess data is lost.
    /// Make sure to always use a sufficiently large buffer to hold the
    /// maximum UDP packet size, which can be up to 65536 bytes in size.
    ///
    /// MacOS will return an error if you pass a zero-sized buffer.
    ///
    /// If you're merely interested in learning the sender of the data at the head of the queue,
    /// try [`peek_sender`].
    ///
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///
    ///     let mut buf = vec![0u8; 32];
    ///     let len = socket.peek(&mut buf).await?;
    ///
    ///     println!("peeked {:?} bytes", len);
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// [`peek_sender`]: method@Self::peek_sender
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    pub async fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.try_peek(buf) {
                Ok(n) => return Ok(n),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::future::poll_fn(|cx| self.poll_recv_ready(cx)).await?;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Receives data from the connected address, without removing it from the input queue.
    ///
    /// # Notes
    ///
    /// Note that on multiple calls to a `poll_*` method in the `recv` direction, only the
    /// `Waker` from the `Context` passed to the most recent call will be scheduled to
    /// receive a wakeup
    ///
    /// On Windows, if the data is larger than the buffer specified, the buffer
    /// is filled with the first part of the data, and peek returns the error
    /// `WSAEMSGSIZE(10040)`. The excess data is lost.
    /// Make sure to always use a sufficiently large buffer to hold the
    /// maximum UDP packet size, which can be up to 65536 bytes in size.
    ///
    /// MacOS will return an error if you pass a zero-sized buffer.
    ///
    /// If you're merely interested in learning the sender of the data at the head of the queue,
    /// try [`poll_peek_sender`].
    ///
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// # Return value
    ///
    /// The function returns:
    ///
    /// * `Poll::Pending` if the socket is not ready to read
    /// * `Poll::Ready(Ok(()))` reads data into `ReadBuf` if the socket is ready
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Errors
    ///
    /// This function may encounter any standard I/O error except `WouldBlock`.
    ///
    /// [`poll_peek_sender`]: method@Self::poll_peek_sender
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    pub fn poll_peek(&self, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let mut borrow = self.peeked.borrow_mut();
        std::task::ready!(self.poll_fill_peeked(cx, &mut borrow))?;
        let bytes = &borrow.as_ref().unwrap().bytes;
        let len = bytes.len().min(buf.remaining());
        buf.put_slice(&bytes[..len]);
        Poll::Ready(Ok(()))
    }

    /// Tries to receive data on the connected address without removing it from the input queue.
    /// On success, returns the number of bytes read.
    ///
    /// When there is no pending data, `Err(io::ErrorKind::WouldBlock)` is
    /// returned. This function is usually paired with `readable()`.
    ///
    /// # Notes
    ///
    /// On Windows, if the data is larger than the buffer specified, the buffer
    /// is filled with the first part of the data, and peek returns the error
    /// `WSAEMSGSIZE(10040)`. The excess data is lost.
    /// Make sure to always use a sufficiently large buffer to hold the
    /// maximum UDP packet size, which can be up to 65536 bytes in size.
    ///
    /// MacOS will return an error if you pass a zero-sized buffer.
    ///
    /// If you're merely interested in learning the sender of the data at the head of the queue,
    /// try [`try_peek_sender`].
    ///
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// [`try_peek_sender`]: method@Self::try_peek_sender
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    pub fn try_peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut borrow = self.peeked.borrow_mut();
        self.try_fill_peeked(&mut borrow)?;
        let bytes = &borrow.as_ref().unwrap().bytes;
        let len = bytes.len().min(buf.len());
        buf[..len].copy_from_slice(&bytes[..len]);
        Ok(len)
    }

    /// Receives data from the socket, without removing it from the input queue.
    /// On success, returns the number of bytes read and the address from whence
    /// the data came.
    ///
    /// # Notes
    ///
    /// On Windows, if the data is larger than the buffer specified, the buffer
    /// is filled with the first part of the data, and `peek_from` returns the error
    /// `WSAEMSGSIZE(10040)`. The excess data is lost.
    /// Make sure to always use a sufficiently large buffer to hold the
    /// maximum UDP packet size, which can be up to 65536 bytes in size.
    ///
    /// MacOS will return an error if you pass a zero-sized buffer.
    ///
    /// If you're merely interested in learning the sender of the data at the head of the queue,
    /// try [`peek_sender`].
    ///
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    ///
    ///     let mut buf = vec![0u8; 32];
    ///     let (len, addr) = socket.peek_from(&mut buf).await?;
    ///
    ///     println!("peeked {:?} bytes from {:?}", len, addr);
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// [`peek_sender`]: method@Self::peek_sender
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    pub async fn peek_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        loop {
            match self.try_peek_from(buf) {
                Ok(res) => return Ok(res),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::future::poll_fn(|cx| self.poll_recv_ready(cx)).await?;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Receives data from the socket, without removing it from the input queue.
    /// On success, returns the sending address of the datagram.
    ///
    /// # Notes
    ///
    /// Note that on multiple calls to a `poll_*` method in the `recv` direction, only the
    /// `Waker` from the `Context` passed to the most recent call will be scheduled to
    /// receive a wakeup
    ///
    /// On Windows, if the data is larger than the buffer specified, the buffer
    /// is filled with the first part of the data, and peek returns the error
    /// `WSAEMSGSIZE(10040)`. The excess data is lost.
    /// Make sure to always use a sufficiently large buffer to hold the
    /// maximum UDP packet size, which can be up to 65536 bytes in size.
    ///
    /// MacOS will return an error if you pass a zero-sized buffer.
    ///
    /// If you're merely interested in learning the sender of the data at the head of the queue,
    /// try [`poll_peek_sender`].
    ///
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// # Return value
    ///
    /// The function returns:
    ///
    /// * `Poll::Pending` if the socket is not ready to read
    /// * `Poll::Ready(Ok(addr))` reads data from `addr` into `ReadBuf` if the socket is ready
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Errors
    ///
    /// This function may encounter any standard I/O error except `WouldBlock`.
    ///
    /// [`poll_peek_sender`]: method@Self::poll_peek_sender
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    pub fn poll_peek_from(
        &self,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<SocketAddr>> {
        let mut borrow = self.peeked.borrow_mut();
        std::task::ready!(self.poll_fill_peeked(cx, &mut borrow))?;
        let peeked = borrow.as_ref().unwrap();
        let from = peeked.packet.header().get_src_addr();
        let bytes = &peeked.bytes;
        let len = bytes.len().min(buf.remaining());
        buf.put_slice(&bytes[..len]);
        Poll::Ready(Ok(from))
    }

    /// Tries to receive data on the socket without removing it from the input queue.
    /// On success, returns the number of bytes read and the sending address of the
    /// datagram.
    ///
    /// When there is no pending data, `Err(io::ErrorKind::WouldBlock)` is
    /// returned. This function is usually paired with `readable()`.
    ///
    /// # Notes
    ///
    /// On Windows, if the data is larger than the buffer specified, the buffer
    /// is filled with the first part of the data, and peek returns the error
    /// `WSAEMSGSIZE(10040)`. The excess data is lost.
    /// Make sure to always use a sufficiently large buffer to hold the
    /// maximum UDP packet size, which can be up to 65536 bytes in size.
    ///
    /// MacOS will return an error if you pass a zero-sized buffer.
    ///
    /// If you're merely interested in learning the sender of the data at the head of the queue,
    /// try [`try_peek_sender`].
    ///
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// [`try_peek_sender`]: method@Self::try_peek_sender
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    pub fn try_peek_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let mut borrow = self.peeked.borrow_mut();
        self.try_fill_peeked(&mut borrow)?;
        let peeked = borrow.as_ref().unwrap();
        let from = peeked.packet.header().get_src_addr();
        let bytes = &peeked.bytes;
        let len = bytes.len().min(buf.len());
        buf[..len].copy_from_slice(&bytes[..len]);
        Ok((len, from))
    }

    /// Retrieve the sender of the data at the head of the input queue, waiting if empty.
    ///
    /// This is equivalent to calling [`peek_from`] with a zero-sized buffer,
    /// but suppresses the `WSAEMSGSIZE` error on Windows and the "invalid argument" error on macOS.
    ///
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// [`peek_from`]: method@Self::peek_from
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    pub async fn peek_sender(&self) -> io::Result<SocketAddr> {
        loop {
            match self.peek_sender_inner() {
                Ok(addr) => return Ok(addr),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::future::poll_fn(|cx| self.poll_recv_ready(cx)).await?;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Retrieve the sender of the data at the head of the input queue,
    /// scheduling a wakeup if empty.
    ///
    /// This is equivalent to calling [`poll_peek_from`] with a zero-sized buffer,
    /// but suppresses the `WSAEMSGSIZE` error on Windows and the "invalid argument" error on macOS.
    ///
    /// # Notes
    ///
    /// Note that on multiple calls to a `poll_*` method in the `recv` direction, only the
    /// `Waker` from the `Context` passed to the most recent call will be scheduled to
    /// receive a wakeup.
    ///
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// [`poll_peek_from`]: method@Self::poll_peek_from
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    pub fn poll_peek_sender(&self, cx: &mut Context<'_>) -> Poll<io::Result<SocketAddr>> {
        let mut borrow = self.peeked.borrow_mut();
        std::task::ready!(self.poll_fill_peeked(cx, &mut borrow))?;
        Poll::Ready(Ok(borrow.as_ref().unwrap().packet.header().get_src_addr()))
    }

    /// Try to retrieve the sender of the data at the head of the input queue.
    ///
    /// When there is no pending data, `Err(io::ErrorKind::WouldBlock)` is
    /// returned. This function is usually paired with `readable()`.
    ///
    /// Note that the socket address **cannot** be implicitly trusted, because it is relatively
    /// trivial to send a UDP datagram with a spoofed origin in a [packet injection attack].
    /// Because UDP is stateless and does not validate the origin of a packet,
    /// the attacker does not need to be able to intercept traffic in order to interfere.
    /// It is important to be aware of this when designing your application-level protocol.
    ///
    /// [packet injection attack]: https://en.wikipedia.org/wiki/Packet_injection
    pub fn try_peek_sender(&self) -> io::Result<SocketAddr> {
        self.peek_sender_inner()
    }

    #[inline]
    fn peek_sender_inner(&self) -> io::Result<SocketAddr> {
        let mut borrow = self.peeked.borrow_mut();
        self.try_fill_peeked(&mut borrow)?;
        Ok(borrow.as_ref().unwrap().packet.header().get_src_addr())
    }

    /// Gets the value of the `SO_BROADCAST` option for this socket.
    ///
    /// For more information about this option, see [`set_broadcast`].
    ///
    /// [`set_broadcast`]: method@Self::set_broadcast
    pub fn broadcast(&self) -> io::Result<bool> {
        unimplemented!()
    }

    /// Sets the value of the `SO_BROADCAST` option for this socket.
    ///
    /// When enabled, this socket is allowed to send packets to a broadcast
    /// address.
    pub fn set_broadcast(&self, _on: bool) -> io::Result<()> {
        unimplemented!()
    }

    /// Gets the value of the `IP_MULTICAST_LOOP` option for this socket.
    ///
    /// For more information about this option, see [`set_multicast_loop_v4`].
    ///
    /// [`set_multicast_loop_v4`]: method@Self::set_multicast_loop_v4
    pub fn multicast_loop_v4(&self) -> io::Result<bool> {
        unimplemented!()
    }

    /// Sets the value of the `IP_MULTICAST_LOOP` option for this socket.
    ///
    /// If enabled, multicast packets will be looped back to the local socket.
    ///
    /// # Note
    ///
    /// This may not have any effect on IPv6 sockets.
    pub fn set_multicast_loop_v4(&self, _on: bool) -> io::Result<()> {
        unimplemented!()
    }

    /// Gets the value of the `IP_MULTICAST_TTL` option for this socket.
    ///
    /// For more information about this option, see [`set_multicast_ttl_v4`].
    ///
    /// [`set_multicast_ttl_v4`]: method@Self::set_multicast_ttl_v4
    pub fn multicast_ttl_v4(&self) -> io::Result<u32> {
        unimplemented!()
    }

    /// Sets the value of the `IP_MULTICAST_TTL` option for this socket.
    ///
    /// Indicates the time-to-live value of outgoing multicast packets for
    /// this socket. The default value is 1 which means that multicast packets
    /// don't leave the local network unless explicitly requested.
    ///
    /// # Note
    ///
    /// This may not have any effect on IPv6 sockets.
    pub fn set_multicast_ttl_v4(&self, _ttl: u32) -> io::Result<()> {
        unimplemented!()
    }

    /// Gets the value of the `IPV6_MULTICAST_LOOP` option for this socket.
    ///
    /// For more information about this option, see [`set_multicast_loop_v6`].
    ///
    /// [`set_multicast_loop_v6`]: method@Self::set_multicast_loop_v6
    pub fn multicast_loop_v6(&self) -> io::Result<bool> {
        unimplemented!()
    }

    /// Sets the value of the `IPV6_MULTICAST_LOOP` option for this socket.
    ///
    /// Controls whether this socket sees the multicast packets it sends itself.
    ///
    /// # Note
    ///
    /// This may not have any effect on IPv4 sockets.
    pub fn set_multicast_loop_v6(&self, _on: bool) -> io::Result<()> {
        unimplemented!()
    }

    /// Gets the value of the `IPV6_TCLASS` option for this socket.
    ///
    /// For more information about this option, see [`set_tclass_v6`].
    ///
    /// [`set_tclass_v6`]: Self::set_tclass_v6
    // https://docs.rs/socket2/0.6.1/src/socket2/sys/unix.rs.html#2541
    #[cfg(any(
        target_os = "android",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "fuchsia",
        target_os = "linux",
        target_os = "macos",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "cygwin",
    ))]
    #[cfg_attr(
        docsrs,
        doc(cfg(any(
            target_os = "android",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "fuchsia",
            target_os = "linux",
            target_os = "macos",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "cygwin",
        )))
    )]
    pub fn tclass_v6(&self) -> io::Result<u32> {
        unimplemented!()
    }

    /// Sets the value for the `IPV6_TCLASS` option on this socket.
    ///
    /// Specifies the traffic class field that is used in every packet
    /// sent from this socket.
    ///
    /// # Note
    ///
    /// This may not have any effect on IPv4 sockets.
    // https://docs.rs/socket2/0.6.1/src/socket2/sys/unix.rs.html#2566
    #[cfg(any(
        target_os = "android",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "fuchsia",
        target_os = "linux",
        target_os = "macos",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "cygwin",
    ))]
    #[cfg_attr(
        docsrs,
        doc(cfg(any(
            target_os = "android",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "fuchsia",
            target_os = "linux",
            target_os = "macos",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "cygwin",
        )))
    )]
    pub fn set_tclass_v6(&self, _tclass: u32) -> io::Result<()> {
        unimplemented!()
    }

    /// Gets the value of the `IP_TTL` option for this socket.
    ///
    /// For more information about this option, see [`set_ttl`].
    ///
    /// [`set_ttl`]: method@Self::set_ttl
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// # use std::io;
    ///
    /// # async fn dox() -> io::Result<()> {
    /// let sock = UdpSocket::bind("127.0.0.1:8080").await?;
    ///
    /// println!("{:?}", sock.ttl()?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn ttl(&self) -> io::Result<u32> {
        unimplemented!()
    }

    /// Sets the value for the `IP_TTL` option on this socket.
    ///
    /// This value sets the time-to-live field that is used in every packet sent
    /// from this socket.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// # use std::io;
    ///
    /// # async fn dox() -> io::Result<()> {
    /// let sock = UdpSocket::bind("127.0.0.1:8080").await?;
    /// sock.set_ttl(60)?;
    ///
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_ttl(&self, _ttl: u32) -> io::Result<()> {
        unimplemented!()
    }

    /// Gets the value of the `IP_TOS` option for this socket.
    ///
    /// For more information about this option, see [`set_tos_v4`].
    ///
    /// [`set_tos_v4`]: Self::set_tos_v4
    // https://docs.rs/socket2/0.6.1/src/socket2/socket.rs.html#1585
    #[cfg(not(any(
        target_os = "fuchsia",
        target_os = "redox",
        target_os = "solaris",
        target_os = "illumos",
        target_os = "haiku"
    )))]
    #[cfg_attr(
        docsrs,
        doc(cfg(not(any(
            target_os = "fuchsia",
            target_os = "redox",
            target_os = "solaris",
            target_os = "illumos",
            target_os = "haiku"
        ))))
    )]
    pub fn tos_v4(&self) -> io::Result<u32> {
        unimplemented!()
    }

    /// Deprecated. Use [`tos_v4()`] instead.
    ///
    /// [`tos_v4()`]: Self::tos_v4
    #[deprecated(
        note = "`tos` related methods have been renamed `tos_v4` since they are IPv4-specific."
    )]
    #[doc(hidden)]
    #[cfg(not(any(
        target_os = "fuchsia",
        target_os = "redox",
        target_os = "solaris",
        target_os = "illumos",
        target_os = "haiku"
    )))]
    #[cfg_attr(
        docsrs,
        doc(cfg(not(any(
            target_os = "fuchsia",
            target_os = "redox",
            target_os = "solaris",
            target_os = "illumos",
            target_os = "haiku"
        ))))
    )]
    pub fn tos(&self) -> io::Result<u32> {
        unimplemented!()
    }

    /// Sets the value for the `IP_TOS` option on this socket.
    ///
    /// This value sets the type-of-service field that is used in every packet
    /// sent from this socket.
    ///
    /// # Note
    ///
    /// - This may not have any effect on IPv6 sockets.
    /// - On Windows, `IP_TOS` is only supported on [Windows 8+ or
    ///   Windows Server 2012+.](https://docs.microsoft.com/en-us/windows/win32/winsock/ipproto-ip-socket-options)
    // https://docs.rs/socket2/0.6.1/src/socket2/socket.rs.html#1566
    #[cfg(not(any(
        target_os = "fuchsia",
        target_os = "redox",
        target_os = "solaris",
        target_os = "illumos",
        target_os = "haiku"
    )))]
    #[cfg_attr(
        docsrs,
        doc(cfg(not(any(
            target_os = "fuchsia",
            target_os = "redox",
            target_os = "solaris",
            target_os = "illumos",
            target_os = "haiku"
        ))))
    )]
    pub fn set_tos_v4(&self, _tos: u32) -> io::Result<()> {
        unimplemented!()
    }

    /// Deprecated. Use [`set_tos_v4()`] instead.
    ///
    /// [`set_tos_v4()`]: Self::set_tos_v4
    #[deprecated(
        note = "`tos` related methods have been renamed `tos_v4` since they are IPv4-specific."
    )]
    #[doc(hidden)]
    #[cfg(not(any(
        target_os = "fuchsia",
        target_os = "redox",
        target_os = "solaris",
        target_os = "illumos",
        target_os = "haiku"
    )))]
    #[cfg_attr(
        docsrs,
        doc(cfg(not(any(
            target_os = "fuchsia",
            target_os = "redox",
            target_os = "solaris",
            target_os = "illumos",
            target_os = "haiku"
        ))))
    )]
    pub fn set_tos(&self, _tos: u32) -> io::Result<()> {
        unimplemented!()
    }

    /// Gets the value for the `SO_BINDTODEVICE` option on this socket
    ///
    /// This value gets the socket-bound device's interface name.
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux",))]
    #[cfg_attr(
        docsrs,
        doc(cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux",)))
    )]
    pub fn device(&self) -> io::Result<Option<Vec<u8>>> {
        unimplemented!()
    }

    /// Sets the value for the `SO_BINDTODEVICE` option on this socket
    ///
    /// If a socket is bound to an interface, only packets received from that
    /// particular interface are processed by the socket. Note that this only
    /// works for some socket types, particularly `AF_INET` sockets.
    ///
    /// If `interface` is `None` or an empty string it removes the binding.
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    #[cfg_attr(
        docsrs,
        doc(cfg(all(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))))
    )]
    pub fn bind_device(&self, interface: Option<&[u8]>) -> io::Result<()> {
        unimplemented!()
    }

    /// Executes an operation of the `IP_ADD_MEMBERSHIP` type.
    ///
    /// This function specifies a new multicast group for this socket to join.
    /// The address must be a valid multicast address, and `interface` is the
    /// address of the local interface with which the system should join the
    /// multicast group. If it's equal to `INADDR_ANY` then an appropriate
    /// interface is chosen by the system.
    pub fn join_multicast_v4(&self, _multiaddr: Ipv4Addr, _interface: Ipv4Addr) -> io::Result<()> {
        unimplemented!()
    }

    /// Executes an operation of the `IPV6_ADD_MEMBERSHIP` type.
    ///
    /// This function specifies a new multicast group for this socket to join.
    /// The address must be a valid multicast address, and `interface` is the
    /// index of the interface to join/leave (or 0 to indicate any interface).
    pub fn join_multicast_v6(&self, _multiaddr: &Ipv6Addr, _interface: u32) -> io::Result<()> {
        unimplemented!()
    }

    /// Executes an operation of the `IP_DROP_MEMBERSHIP` type.
    ///
    /// For more information about this option, see [`join_multicast_v4`].
    ///
    /// [`join_multicast_v4`]: method@Self::join_multicast_v4
    pub fn leave_multicast_v4(&self, _multiaddr: Ipv4Addr, _interface: Ipv4Addr) -> io::Result<()> {
        unimplemented!()
    }

    /// Executes an operation of the `IPV6_DROP_MEMBERSHIP` type.
    ///
    /// For more information about this option, see [`join_multicast_v6`].
    ///
    /// [`join_multicast_v6`]: method@Self::join_multicast_v6
    pub fn leave_multicast_v6(&self, _multiaddr: &Ipv6Addr, _interface: u32) -> io::Result<()> {
        unimplemented!()
    }

    /// Returns the value of the `SO_ERROR` option.
    ///
    /// # Examples
    /// ```no_run
    /// use dens::os_mock::net::UdpSocket;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    /// #   if cfg!(miri) { return Ok(()); } // No `socket` in miri.
    ///     // Create a socket
    ///     let socket = UdpSocket::bind("0.0.0.0:8080").await?;
    ///
    ///     if let Ok(Some(err)) = socket.take_error() {
    ///         println!("Got error: {:?}", err);
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        unimplemented!()
    }
}

impl TryFrom<std::net::UdpSocket> for UdpSocket {
    type Error = io::Error;

    /// Consumes stream, returning the tokio I/O object.
    ///
    /// This is equivalent to
    /// [`UdpSocket::from_std(stream)`](UdpSocket::from_std).
    fn try_from(stream: std::net::UdpSocket) -> Result<Self, Self::Error> {
        Self::from_std(stream)
    }
}

#[allow(clippy::missing_fields_in_debug)]
impl fmt::Debug for UdpSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UdpSocket")
            .field("local_addr", &self.local_addr)
            .finish()
    }
}

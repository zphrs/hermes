use std::{
    cmp::Ordering,
    hash::Hash,
    marker::PhantomData,
    net::{IpAddr, SocketAddr},
    str::FromStr,
    thread::spawn,
};

use capnp::message::{Builder, HeapAllocator, ReaderOptions};

use capnp_futures::serialize::AsOutputSegments;
use futures_util::{StreamExt, io::AsyncReadExt};
use tokio::{runtime::Handle, task::LocalSet};
use tokio_util::compat::TokioAsyncReadCompatExt as _;
use tracing::error;

use crate::{
    listener::Listener,
    sky_capnp::{self, address, client_intro, client_return, connect_request, ip},
};

#[derive(Clone, Copy)]
pub struct Server<L: Listener> {
    listener: PhantomData<L>,
}

pub struct Client<'a, L: Listener> {
    address: Address<'a>,
    invisible: bool,
    sender: capnp_futures::Sender<Builder<HeapAllocator>>,
    read_stream: capnp_futures::ReadStream<
        'a,
        futures_util::io::ReadHalf<tokio_util::compat::Compat<<L as Listener>::Io>>,
    >,
}

impl<'a, L: Listener> Client<'a, L> {
    pub fn new(
        address: Address<'a>,
        invisible: bool,
        sender: capnp_futures::Sender<Builder<HeapAllocator>>,
        read_stream: capnp_futures::ReadStream<
            'a,
            futures_util::io::ReadHalf<tokio_util::compat::Compat<<L as Listener>::Io>>,
        >,
    ) -> Self {
        Self {
            address,
            invisible,
            sender,
            read_stream,
        }
    }
    // returns whether the stream is still open
    pub async fn handle_next_client_request(&mut self) -> Result<bool, capnp::Error> {
        let Some(next) = self.read_stream.next().await else {
            return Ok(false);
        };

        let next = next?;

        let req = next.get_root::<connect_request::Reader<'_>>()?;
        let connect_to: Address = req.get_to()?.try_into()?;
        Ok(true)
    }

    pub fn send_client_init_conn(&self) {}
}
#[derive(Clone, Copy)]
pub struct Node {
    addr: SocketAddr,
}

impl Node {
    pub fn new(socket_addr: SocketAddr) -> Self {
        Self { addr: socket_addr }
    }
}

pub trait InitWith<'a, T>
where
    Self: capnp::traits::FromPointerBuilder<'a>,
{
    fn init_with(self, other: &T) -> Result<(), capnp::Error>;
}

pub trait FillBuilder<'a, CapnpType> {
    fn fill_builder(&self, builder: CapnpType) -> Result<(), capnp::Error>;
}

impl<'a, CapnpType: InitWith<'a, T>, T> FillBuilder<'a, CapnpType> for T {
    fn fill_builder(&self, builder: CapnpType) -> Result<(), capnp::Error> {
        builder.init_with(self)
    }
}

impl<'a> InitWith<'a, Node> for sky_capnp::node::Builder<'a> {
    fn init_with(self, other: &Node) -> Result<(), capnp::Error> {
        self.init_addr().init_with(&other.addr)?;
        Ok(())
    }
}

impl<'a> InitWith<'a, SocketAddr> for sky_capnp::socket_addr::Builder<'a> {
    fn init_with(mut self, other: &SocketAddr) -> Result<(), capnp::Error> {
        self.reborrow().init_ip().init_from_ip_addr(other.ip())?;
        self.set_port(other.port());
        Ok(())
    }
}

pub enum Address<'a> {
    Bytes(&'a [u8]),
    Reader(address::Reader<'a>),
}

impl PartialEq for Address<'_> {
    fn eq(&self, other: &Self) -> bool {
        let self_iter = self.iter();
        let other_iter = other.iter();
        if self_iter.len() != other_iter.len() {
            return false;
        };

        for (a, b) in self_iter.zip(other_iter) {
            if a != b {
                return false;
            };
        }
        return true;
    }
}

impl Eq for Address<'_> {}

impl Ord for Address<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let self_iter = self.iter();
        let self_len = self_iter.len();
        let other_iter = other.iter();
        let other_len = other_iter.len();
        let zipped = self_iter.zip(other_iter);
        for (a, b) in zipped {
            match a.cmp(&b) {
                Ordering::Less => return Ordering::Less,
                Ordering::Equal => continue,
                Ordering::Greater => return Ordering::Greater,
            }
        }
        // if first n bytes are equal,
        return self_len.cmp(&other_len);
    }
}

impl PartialOrd for Address<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for Address<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(&self.iter().collect::<Vec<u8>>())
    }
}

impl<'a> Address<'a> {
    pub fn iter(&self) -> Box<dyn ExactSizeIterator<Item = u8> + 'a> {
        match self {
            Address::Bytes(items) => Box::new(items.iter().copied()),
            Address::Reader(reader) => Box::new(reader.get_bytes().unwrap().iter()),
        }
    }
}

impl<'a> From<&'a [u8]> for Address<'a> {
    fn from(value: &'a [u8]) -> Self {
        Self::Bytes(value)
    }
}

pub struct ClientIntro<'a> {
    address: Address<'a>,
    invisible: bool,
}

impl<'a, 'b> FillBuilder<'a, client_intro::Builder<'b>> for ClientIntro<'a>
where
    'b: 'a,
{
    fn fill_builder(&self, mut builder: client_intro::Builder<'b>) -> Result<(), capnp::Error> {
        match self.address {
            Address::Bytes(items) => builder.reborrow().init_address().set_bytes(items)?,
            Address::Reader(reader) => builder.set_address(reader)?,
        };
        builder.set_invisible(self.invisible);
        Ok(())
    }
}

pub async fn build_client_message(address: Address<'_>, invisible: bool) {
    let mut msg = Builder::new_default();
    let mut client = msg.init_root::<client_intro::Builder>();
    client.set_invisible(invisible);
    match address {
        Address::Bytes(items) => {
            // copies
            client.init_address().set_bytes(items).unwrap()
        }
        Address::Reader(reader) => {
            client.set_address(reader).unwrap();
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("cap'n Proto error: {0}")]
    Capnp(#[from] capnp::Error),
    #[error("no initial client message")]
    NoInitMessage,
}

impl<L: Listener> Server<L> {
    pub async fn listen_client(&self, listener: &mut L) {
        loop {
            let conn = listener.accept().await;
            std::thread::spawn(move || {
                Handle::current().block_on(async move {
                    let client = Self::handle_client_conn(conn).await;
                    let client = match client {
                        Err(e) => {
                            error!("{e}");
                            return;
                        }
                        Ok(client) => client,
                    };
                });
            });
        }
    }
    pub async fn handle_client_conn((socket, local_addr): (L::Io, L::Addr)) -> Result<(), Error> {
        let (socket_read, socket_write) = socket.compat().split();
        let mut read_stream = capnp_futures::ReadStream::new(
            socket_read,
            *ReaderOptions::new().traversal_limit_in_words(Some(8)),
        );
        let future_next = read_stream.next();

        let next_item = future_next.await.ok_or(Error::NoInitMessage)??;

        let intro = next_item.get_root::<client_intro::Reader>()?;
        let address: Address<'_> = intro.get_address()?.try_into()?;
        let invisible: bool = intro.get_invisible();
        let mut msg = Builder::new_default();

        let root = msg.init_root::<client_return::Builder>();
        let is_my_address = /* TODO */ true;
        if !is_my_address {
            let redirect = root.init_redirect();
            let redirect_to = Node::new(SocketAddr::new(
                IpAddr::from_str("127.0.0.1").unwrap(),
                8080,
            ));
            redirect_to.fill_builder(redirect)?;
        } else {
            let nodes: Vec<Node> = Vec::new();
            let mut ok = root.init_ok(nodes.len() as u32);
            for (i, node) in nodes.iter().enumerate() {
                ok.reborrow().get(i as u32).init_with(node)?;
            }
        }

        let (sender, task): (capnp_futures::Sender<Builder<HeapAllocator>>, _) =
            capnp_futures::write_queue(socket_write);

        Handle::current().spawn(task);

        let client = Client::<L>::new(address, invisible, sender, read_stream);

        Ok(())
    }
}

impl<'a> TryFrom<address::Reader<'a>> for Address<'a> {
    type Error = capnp::Error;

    fn try_from(value: address::Reader<'a>) -> Result<Self, Self::Error> {
        let address = Address::Reader(value);
        Ok(address)
    }
}

impl<'a> ip::Builder<'a> {
    pub fn init_from_ip_addr(mut self, ip: IpAddr) -> Result<(), capnp::Error> {
        match ip {
            IpAddr::V4(ipv4_addr) => self.set_v4(ipv4_addr.into()),
            IpAddr::V6(ipv6_addr) => self.set_v6(&ipv6_addr.octets())?,
        };
        Ok(())
    }
}

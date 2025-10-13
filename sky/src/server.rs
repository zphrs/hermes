use std::{
    marker::PhantomData,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    str::FromStr,
};

use capnp::message::{Builder, HeapAllocator, ReaderOptions};

use futures_util::{StreamExt, io::AsyncReadExt};
use kademlia::RpcManager;
use tokio::{net::TcpStream, runtime::Handle};
use tokio_util::compat::TokioAsyncReadCompatExt as _;
use tracing::error;

use crate::{
    capnp_types::{FillBuilder as _, FillMessage, FindNodeRequest, Id, InitWith as _, PeerRequest},
    listener::Listener,
    node::Node,
    sky_capnp::{
        self, client_intro, client_return, connect_request, find_node_response, peer_request,
        ping_response,
    },
};

#[derive(Clone, Copy)]
pub struct KadReqHandler;

impl kademlia::RequestHandler<Node, 32> for KadReqHandler {
    // we expect it to be from our machine
    async fn ping(&self, _from: &Node, node: &Node) -> bool {
        let node_addr = node.addr();

        // open socket
        let Ok(s) = TcpStream::connect(node_addr).await else {
            return false;
        };

        // Convert to compat types for capnp
        let s = s.compat();
        let (mut read_half, mut write_half) = futures_util::io::AsyncReadExt::split(s);

        // Write ping request
        let mut msg = Builder::new_default();
        PeerRequest::Ping.fill_builder(msg.init_root::<peer_request::Builder>());
        PeerRequest::Ping.fill_message::<peer_request::Builder>(&mut msg);

        // Serialize and send the message
        if capnp_futures::serialize_packed::write_message(&mut write_half, &msg)
            .await
            .is_err()
        {
            return false;
        }

        // Read response
        let Ok(message_reader) =
            capnp_futures::serialize_packed::read_message(&mut read_half, ReaderOptions::new())
                .await
        else {
            return false;
        };

        // Parse response
        let Ok(_ping_resp) = message_reader.get_root::<ping_response::Reader>() else {
            return false;
        };

        true
    }

    async fn find_node(&self, from: &Node, to: &Node, address: &Id) -> Vec<Node> {
        let node_addr = to.addr();

        // open socket
        let Ok(s) = TcpStream::connect(node_addr).await else {
            return vec![];
        };

        // Convert to compat types for capnp
        let s = s.compat();
        let (mut read_half, mut write_half) = futures_util::io::AsyncReadExt::split(s);

        // Write ping request
        let mut msg = Builder::new_default();
        &PeerRequest::FindNode(FindNodeRequest::new(address.clone()))
            .fill_message::<peer_request::Builder>(&mut msg);

        // Serialize and send the message
        if capnp_futures::serialize_packed::write_message(&mut write_half, &msg)
            .await
            .is_err()
        {
            return vec![];
        }

        // Read response
        let Ok(message_reader) =
            capnp_futures::serialize_packed::read_message(&mut read_half, ReaderOptions::new())
                .await
        else {
            return vec![];
        };

        // Parse response
        let Ok(find_node_response) = message_reader.get_root::<find_node_response::Reader>() else {
            return vec![];
        };

        let Ok(nodes) = find_node_response.get_nodes() else {
            return vec![];
        };
        Vec::from_iter(nodes.iter().filter_map(|node| Node::try_from(node).ok()))
    }
}
#[derive(Clone)]
pub struct Server<L: Listener> {
    listener: PhantomData<L>,
    pub kad: RpcManager<Node, KadReqHandler, 32, 20>,
}

pub struct Client<'a, L: Listener> {
    address: Id,
    invisible: bool,
    sender: capnp_futures::Sender<Builder<HeapAllocator>>,
    read_stream: capnp_futures::ReadStream<
        'a,
        futures_util::io::ReadHalf<tokio_util::compat::Compat<<L as Listener>::Io>>,
    >,
}

impl<'a, L: Listener> Client<'a, L> {
    pub fn new(
        address: Id,
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
        let connect_to: Id = req.get_to()?.into();
        Ok(true)
    }

    pub fn send_client_init_conn(&self) {}
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("cap'n Proto error: {0}")]
    Capnp(#[from] capnp::Error),
    #[error("no initial client message")]
    NoInitMessage,
}

impl<L: Listener + Send + Sync + Clone> Server<L> {
    pub async fn listen_client(&self, listener: &mut L) {
        loop {
            let conn = listener.accept().await;
            let cloned_self = (*self).clone();
            std::thread::spawn(move || {
                Handle::current().block_on(async move {
                    match cloned_self.handle_client_conn(conn).await {
                        Err(e) => {
                            error!("{e}");
                            return;
                        }
                        Ok(client) => {}
                    };
                });
            });
        }
    }
    pub async fn handle_client_conn(
        &self,
        (socket, _local_addr): (L::Io, L::Addr),
    ) -> Result<(), Error> {
        let (socket_read, socket_write) = socket.compat().split();
        let mut read_stream = capnp_futures::ReadStream::new(
            socket_read,
            *ReaderOptions::new().traversal_limit_in_words(Some(8)),
        );
        let future_next = read_stream.next();

        let next_item = future_next.await.ok_or(Error::NoInitMessage)??;

        let intro = next_item.get_root::<client_intro::Reader>()?;
        let address: Id = intro.get_address()?.into();
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
                ok.reborrow().get(i as u32).with(node)?;
            }
        }

        let (sender, task): (capnp_futures::Sender<Builder<HeapAllocator>>, _) =
            capnp_futures::write_queue(socket_write);

        Handle::current().spawn(task);

        let client = Client::<L>::new(address, invisible, sender, read_stream);

        Ok(())
    }
}

# The Sky

The sky is a service based on a hashring. The hashring has nodes based on from
IP addresses (or domain names). Clients of the sky connect to the sky for two
purposes; one to advertise that they are online, and two to send requests to
initiate NAT hole punched connections to other nodes.

# Server Steps

First the server will read from a file all of the server's previously known
peers.

## On Connection

The server will wait for the init message which if a client will announce the
client's address id and whether they are an invisible client or if a server will
send along a list of all known servers thus far.

It is assumed that nodes at the same address and listening on the same port are
in fact the same peer.

## Connections from Clients

On connection the server will wait for a message from the client where the
client announces their address id and whether they are an invisible client. If
the client's address id is after this node and this node does not know of
another node which is after this node and before the client's id then this node
will handle the addressing of this client. Otherwise, the node will redirect the
client to the node which it knows is closest to the target node.

If this node believes it is closest of the nodes it knows of: REPLY
`OK: [list of all sky nodes the node knows of, including itself]`

If this node believes a different node is closer: REPLY
`REDIRECT TO <(EITHER DOMAIN NAME OR IP ADDRESS OF NEAREST KNOWN CLIENT):PORT>`

If the client is invisible then it shouldn't show up on lookups from other
clients unless the lookup is for exactly their address.

Since the connection to the sky is by definition persistent, if a connection to
a sky node is dropped (or times out) from the client then the client should
simply retry their connection to the sky, possibly attempting to connect to a
different sky node it is aware of.

When a server comes online it should also try to connect to other previously
known sky nodes to torrent the list of sky nodes it is aware of.

Since the clients are ephemeral, the only data which sky nodes should persist
and forward is a set of sky nodes' addresses.

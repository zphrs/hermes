# The Earth Layer

The earth is a hash ring of end peer devices, some of which are on metered
connections and others on unmetered connections, which is responsible for
storing messages addressed to a specific inbox for up to H hours. Those on
metered connections will only accept incoming connections from those in their
friend list, those on unmetered connections will altruistically allow any
connection from any peer on the sky network. The users which are only allowing
direct connections from established friends require a handshake from that
attempted connection before accepting any messages which verify that the friend
is indeed a friend. This handshake is sent via the sky layer, alongside the
requesting connector's IP and port. While theoretically all peers connected to
the earth network are publicly known (in the sense that a node with a specific
user ID is online at a specific time and connected to a specific sky node), the
sky will only actively publish the online status of nodes which are on an
unmetered network to earth nodes looking to publish to the nearest available
node in the network, and will only publish their current ID, keeping their IP
address hidden except to those who directly communicate with them.

Earth nodes that forward packets have unmetered egress but typically lack
publicly available IP addresses due to NAT layers.

## Message Storage and Replication

Nodes which are on unmetered connections are published to the sky's DHT as
currently active nodes. These nodes will hold onto messages for user IDs near
their own in the hashring until the messages expire (based on a per-message
expiry date), until they ratchet their user ID, or until they go offline,
whichever happens sooner. In the case that a node ratchets their user ID, they
should attempt to forward the data they are storing to the node on the earth
which is immediately behind them. Same goes for before a node goes offline. That
said, by default, nodes will replicate the message up to a finite number of
times in the hashring. This ensures that if one node goes offline then other
nodes on the ring will still be able to deliver the intended user's message.
Furthermore, nodes should maintain a connection to the nodes near them in the
hashring in order to facilitate this sharing of messages and to alert others
when they go offline. When any node is detected to be offline (i.e. doesn't
reply to a ping message from a neighbor), the nodes near to it will then attempt
to replicate all of that node's messages to another node near the end message's
address. This ensures that a roughly constant number of nodes replicate the
message consistently.

## Purpose and Design Philosophy

The entire point of the earth layer, compared to simply centralizing message
storing and delivery, is to distribute the bandwidth obligation across the
capable internet connections of the majority of people who are using the
application from home/an office/a cafe and therefore are able to act as message
keepers for nodes which are expected to come online at a later date. Meanwhile,
users who are refreshing their messages on the go do not have to worry about
running down their data plan by trying to forward along connections. Since most
of the time end users are connected to reliable and unmetered networks, this
isn't much of an issue.

## IP-Based Confidentiality Attacks

Since generally users identities are not publically tied to their public
addresses, getting any generic (public address, IP address) pair isn't useful.
Instead, it becomes useful only when one user is compromised and known to
message other users. Luckily, the earth network will rarely establish a direct
connection between the sender and the receiver, only doing so when both users
are online at the same time. This is because low-latency matters only when both
users are simultaneously online.

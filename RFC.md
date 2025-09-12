# Hermes: Peer to Peer Messaging Service

The various services are the sky, the earth, and the underworld.

The sky is responsible for connecting active users together and providing a lookup
of the online users which are nearby a specific address. It is called the sky
because these servers will often sit in the cloud, even behind networks with
metered throughput, and merely connect nodes sitting in the earth layer to one
another. While any given sky node can also be other roles, this allows maximal
use of the utility of a static ip and an open port for fascilitating the network
while minimizing the consumption of often costly egress fees by relying on the
earth layer to fascilitate message transfers.

The earth is responsible for torrenting messages between users. Since the sky
provides a way for users even behind firewalls to establish peer to peer
connections, most users are able to participate in the earth which does the bulk
of the data transfer between peers. This signficantly reduces server load on
the sky since the sky is merely responsible for routing
clients to one another, not for actually transmitting messages between users.
The earth will be primarily run on non-metered connections such as home wifi
networks. There are APIs on every target platform to detect whether a network is
metered and can be configured per-network in system preferences. While this
setting is often used to set quality of streaming and delay "maintenance"
network traffic like syncing large files in the background, this setting is also
useful to ensure that we only use peers on reliable and cheap networks to
fascilitate third party message delivery.

That said, metered connections should still hook into the earth when online
to get their own messages synced and deliveredhowever they should not assist
other nodes with message transfer. While in the future we might penalize nodes
which never help to fascilitate network transfers, that would require either
breaking the anomynity of the network or creating some form of
anomynity-preserving currency which is largely undesirable due to the overhead
of currency creation, potential forms of inequality, and possible
(arguably unfair) punishment of those who only have access to metered
connections. This would only be necessary in a world where no nodes are willing
to volunteer their connections to fascilitate other users'
messages which I find unlikely since many users in torrenting and in the
cryptocurrency space volunteer computation to assist other systems of
users altruistically.
I especially find it unlikely since network fascilitation is incredibly cheap
in terms of computation and since many users have unmetered networks (including
me personally) I find it likely that most users would be happy fascilitating
third party data transfers knowing that they will likely also use the same service
when they are offline.

The underworld is responsible for managing a user's contacts and groups, sending
acknowledgements to received messages, and deduplicating messages, and is
called the underworld since all of its traffic is end to end encrypted and it
sits underneath the database and the rest of the application stack.

## The Sky in more depth

The sky fascilitates peer-to-peer connections between users. Essentially clients
message the service with their ip address, their port, and a target user ID. Then
the sky will attempt to initialize a connection to that specific user. The sky
is also responsible for disclosing whether a specific user id is online or not.

Constantly disclosing whether or not a user is online to the sky is somewhat a
confidentiality concern. This can be mitigated by having
deterministic rotating keys of a specific user such that a user will regularly
and predictably switch their addressable id based on the current time by
forward-ratcheting their address at a specific known interval. This way old
friends merely need to ratchet their friend's id meanwhile the sky has no
way to ratchet the user's id because the ratcheting algorithm requires a secret
piece of data.

Since different users connect to different sky servers based on their current
id, unless a user gets incredibly unlucky consistently ending up with the same
node operator, the times thay are online will be anonymized.

This can further be anonymized by having the user randomly report that they are
online even when inactive while the program is running in the background through
background wakeups of the application. That said, randomization of sky operators
will likely be good enough at anonymizing user activity provided there are enough
sky operators online.

Furthermore, since sky operators merely fascilitate the connection of two online
users and do not see the actual messages sent, sky operators really only have
information about a specific id at a specific time, especially since user ids
are consistently ratcheting forward.

This works until you have a former friend who a user wants to remove from their
friend list and furthermore prevent that friend from being able to track their
online status. When a friend is removed from a friend list, a user can reliably
broadcast a total change of address to all of their friends. This ensures that
To prevent losing friends from a total address change, a user can repeatedly
broadcast messages to their old friends who have not acknowledged their address
change yet. This broadcasting only needs to be repeated at around the interval
at which messages expire in the earth cache.

The sky is a hashring where nodes are distributed on the hashring based on their
ip address to prevent intentional ip tracking of users through regeneration of
user ids while ensuring load distribution across all participants of the sky.
Furthermore, since it is based on devices' ips there is less metadata
to track and sky nodes' are not able to be spammed with messages since their
node id is simply private within the sky. Lastly, since members of the sky
explicitly are supposed to have open networks which actively invite users to
connect to them directly in order to fascilitate connections, it implicitly
makes sense that their ip address would be the way to accomplish load bearing.

While theoretically users can use any of the sky participants in order to
get connected to other nodes, because users will advertise to their nearby sky
node, it intuitively makes sense to try to connect to your peer by pinging the
sky node near to your peer on the hash ring (based on their current ratcheted
node id, of course). Furthermore, it makes intuitive sense to maintain a
connection to the id near to you because then users trying to establish a peer
to peer connection directly with you will likely only have to try once in order
to find the sky participant which you are connected to. Furthermore,
the sky can also forward connection requests efficiently because users will
connect to the node near to them. This ensures that both users and other sky
participants can find the nearest node in log(n) time, where n is the number
of sky participants.

## The Earth in more depth

The earth is a hash ring of direct peers, some of which are only allowing direct
connections from their established friends, others of which are altruistically
allowing any connection from any user of the network. The users which are only
allowing direct connections from established friends require a handshake from
that attempted connection before allowing messages which authenticates their
friend through public key/private key challenges. This handshake is sent
alongside the peer's ip and port which is attempting to connect directly to them
via the sky. While theoretically all peers connected to the earth network are in
fact publicly known (in the sense that a node with a specific user id and ip
address is online at a specific time), the sky will only actively publish the
online status of nodes which are on a reliable network to earth nodes looking
to publish to the nearest available node in the network. Nodes which are not
on metered connections are indeed published as currently active nodes. These
nodes will hold onto messages for user ids near their own in the hashring until
the messages expire (based on a per-message expiry date) or until they ratchet
their user id, whichever happens sooner. In the case that a node ratchets their
user id, they should attempt to forward the data they are storing to the node
on the earth which is immediately behind them. Same goes for before a node
goes offline. That said, by default, nodes will replicate the message up to a
finite number of nodes behind them in the hashring. This ensures that if one
node goes offline then other nodes on the ring will still be able to deliver
the intended user's message. Furthermore, nodes should maintain a connection to
the nodes near them in the hashring in order to fascilitate this sharing of
messages and to alert others when they go offline. When any user goes offline,
the nodes near to it will then attempt to replicate all of that node's messages
to the next node back in the hashring. This ensures that a constant number of
nodes replicate the message consistently.

The entire point of the earth, compared to simply centralizing message storing
and delivery, is to distribute the bandwidth obligation across the capable
wifi networks of the majority of people who are working from home/an office/
a cafe with wifi and therefore are able to act as message keepers for nodes
which are expected to come online at a later date.

The expiration of a message is simply determined by the node which broadcasted
the message in the first place os heuristically based on the normal longer
gap between logins of the sender of the message. This duration can be minimized
for most users by waking up user devices to sync periodically.

## The Distributables

All three will be distributed as libraries, and both the sky and the earth
will also have binaries which will start the server for the specific
service.

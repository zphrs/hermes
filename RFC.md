# Hermes: Peer-to-Peer Messaging Service

Hermes provides a peer-to-peer messaging service where users send messages to
recipients via 256-bit virtual addresses. Messages persist for H hours before
garbage collection. Recipients can fetch messages anytime within this window
using their virtual address. Since message delivery is not guaranteed on the
asynchronous internet,
[reliable delivery](<https://en.wikipedia.org/wiki/Reliability_(computer_networking)>)
is used: senders rebroadcast messages (up to every H hours) until receiving an
acknowledgement from the recipient.

Every R hours, users rotate their address by hashing their old address with a
secret known only to their friends. Rotating addresses regularly prevents denial
of service, tracking, and metadata leaks, making it harder for non-friends to
build contact lists based on message metadata.

When a user removes someone from their contact list, they broadcast a new
address to remaining friends using reliable delivery (an "address reset"). After
an address reset, users continue checking their old mailbox to handle the edge
case where two friends change addresses simultaneously.

This system allows data transfer between peers that may be:

-   behind Network Address Translation (NAT) layers
-   sporadically online/available
-   on either metered or unmetered networks

To accomplish this, a tri-layered approach is used with three layers: the Sky,
the Earth, and the Underworld. The Sky handles NAT hole punching to facilitate
connections between peers in the Earth layer. The Earth layer performs
best-effort forwarding of messages sent within the last H hours and delivers
messages to intended recipients. Only nodes on unmetered connections store,
forward, and deliver messages not addressed to them. The Underworld handles
dating, signing, encrypting, and reliably delivering messages.

By splitting up the responsibilities of hermes into 3 layers, computers with
different resources can participate in the layers which have those resources
available to them:

Sky nodes are typically in cloud computing environments with static IP addresses
and high uptime but costly egress fees.

Earth nodes that forward packets have unmetered egress (such as most home
internet connections) but typically lack publicly available IP addresses due to
NAT layers.

Underworld nodes are clients' end machines (metered or unmetered) that know the
client's encryption keys and contact list, making them the only nodes that can
determine where, when, and how to send messages to their intended recipients.

## Layer Details

For detailed information about each layer, see:

-   [The Sky Layer](rfc/sky.md) - NAT hole punching and peer connection
    facilitation
-   [The Earth Layer](rfc/earth.md) - Message storage, forwarding, and delivery
-   [The Underworld Layer](rfc/underworld.md) - Message encryption and reliable
    delivery

## Possible Privacy Limitations

When it comes to privacy, the network merely aims to match or do better compared
to existing centralized messaging platforms. That means that while everyone
shouldn't be able to link your IP address with a specific user ID, if a single
party controls most of the sky/earth nodes then yes, your IP address might be
linkable to a set of IDs and that single party might be able to know who else
you are talking to. However, if we assume that most sky/earth nodes are
privacy-preserving and/or are controlled by non-cooperative parties, then the
sky/earth nodes will not be able to aggregate a list of persistent unique users
with specific contacts. This is because user IDs change daily to everyone who
isn't a friend of yours. This is because IP addresses on end machines often have
a one-to-many relationship with a specific node. This is because when a user ID
is randomized daily, the end node will now connect to the earth using both a new
sky node and have a new set of neighboring earth nodes.

### Information Leaking

Since all messages are encrypted, the main privacy concern comes from possible
leaking of your activities; i.e. who you're messaging, how much you're
messaging, when you're messaging, and from what IP address you're messaging.

Who you're messaging is mostly obfuscated by having every user continuously
rotate their public addresses. Notably your friends theoretically will have the
information necessary to track whether you are messaging a specific mutual
friend of yours. I consider this risk acceptable personally since I don't really
care about someone knowing we have a mutual friend. If you do not, have a
different messaging account for each friend. That unfortunately means the number
of incoming mailboxes then scales linearly with the number of friends that you
have. I personally wouldn't accept that cost, but if you do, you can have a
different mailbox for each individual person. Notably, some social media sites
expose mutual friends as a feature, such as Discord.

That said, another way to identify "who" a node is messaging is via the
recipient's (IP address, temporary address) tuple. This is only available when
the recipient is online since the physical IP address of any virtual address is
not needed in order to send messages to the earth node closest to the virtual
address. and is generally only exposed to the earth nodes it comes in contact
with and the sky node it's closest to. While any given earth node might contact
several sky nodes, only the sky node closest to the earth node in the DHT needs
to know the virtual address of the earth node (to facilitate requests to connect
to that earth node). The Sky's DHT only stores the virtual addresses of the
earth nodes, thus the physical address is not shared to all sky nodes. Instead,
only the sky node closest to each earth node will know that earth node's
address, assuming the sky node does not go out of its way to broadcast the IP
address of the connecting nodes.

### Tracking Your IP

Your friends (i.e. those who are in your contact list) will be theoretically
able to track your IP address since ideally the network optimizes for direct
peer to peer hole-punched connections with your friends. This is somewhat
mitigated by only having devices accept direct connections if they are actively
on and being used (i.e. not performing a background notification fetch) since
your IP address would only be known by your friends, assuming they aren't
running a modified client which logs your IP, when both you and your friend is
actively online and messaging live.

If you don't trust your friends with your IP address (which can be used to find
your home address) then you should use a centralised or decentralized VPN like
TOR. I posit that most ordinary people trust the friends they're actively
communicating to, to not run a modded client which will expose your IP address
to them and to not then use your IP address in order to subpoena your place of
residence/report your IP to law enforcement.

As for people not in your friends list, they'd have to control a majority of
either the sky nodes or the earth nodes in order to track a specific user's
temporary addresses as it changes over time. While this is possible, especially
when the network is first starting up, this sort of tracking will become
prohibitively expensive over time. Furthermore, even if a malicious actor was in
control of the majority of the sky or the earth, all they'd be able to track is
a specific user's IP addresses and which other users they message.

### Tracking Message Activity

All messages must clearly state who they're from, the time they were sent, who
they're addressed to, and their size. The time they were sent can be obfuscated
by randomizing when messages get sent throughout the day which increases latency
for enhanced privacy. The size can be obfuscated by padding the message into
clearly sized chunks at the added cost of additional egress. The recipient is
already hard to figure out for anyone who doesn't have the recipient in their
contact list. The sender is similarly obfuscated.

This means that only your direct friends can know who you're messaging if your
direct friends are also direct friends with the intended recipient (and that
they're running a modified client which keeps logs of all messages it sees where
you're the sender).

If you don't want any of your friends to have any chance of knowing that you're
talking to someone they have in their contacts, have a different account for
every friend. This means that messages addressed to you are not aggregated in
one part of the network and thus it will take more effort to fetch data from
multiple locations.

## The Distributables

All three layers will be distributed as libraries. Both the Sky and the Earth
will also have binaries which will start the server for the specific service.
Meanwhile, the Underworld will be kept solely as a library with an FFI, which
can then be called through any language which supports FFIs.

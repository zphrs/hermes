# Denial of Service Performed by a Local Adversary

The two systems which can allow a denial of service by a local adversary are the
sky network which is responsible for connecting earth nodes and the earth
network which is responsible for temporarily storing messages. Theoretically
attacks to both should be addressable by proof-of-work because the resources
available to genuine nodes should always exceed the resources available. This is
because the sky nodes' roles are very simple. Sky nodes simply fascilitate hole
punching. This means that the inputs and outputs to the server are fixed-size
and thus handled in low constant time. Because costs to the sky nodes are brief
and low, we assume that there will always be abundancy across the sky in the
non-adversarial cases. In adversarial cases however,

In constrast, fellow earth nodes fascilitate temporary storage of messages,
where messages can be of an arbitrary size. One option to reduce this to a
constant size is to

## Sky Node Attacks

An attack on the sky network is earth nodes performing several random find_node
requests. The benefit of an attack on the sky node is to prevent any honest
requests to the network. This can be mitigated by a) requiring a fixed cost to a
specific identity and b) imposing a local per-sky node rate limit based on that
fixed cost identity (which allows for bursting).

For most residential and mobile connections, public IP can serve as a costly
identity. In some cases however, one public IP address can be shared by up to
tens of thousands of honest clients through NAT. In these cases we can use proof
of work (PoW) to provide a secondary costly identity as a fallback for rate
limiting without sacrificing the ability to restrict malicious actors and
without sacrificing anomynity.

## Proof of Work

Proof of work is where a sky node sends a challenge to a specific client which
the client must then complete, typically `hash(challenge.concat(nonce))` where
`challenge` is provided by the server and `nonce` is a monotonically increasing
value provided by the client. Once the client finds a nonce value which makes
the first $b$ bits of the hash 0 bits, where b is the current difficulty, the
client sends back the challenge and the found nonce which the server then
verifies. Since a challenge is easy to create and verify the correctness of, it
doesn't significantly burden the sky server. $b$ will be set dynamically based
on the demand which the sky server is facing.

The specific intricacies of the implementation take significant inspiration from
[Tor's PoW system](https://spec.torproject.org/hspow-spec/index.html) with one
notable difference.

Tor's system uses an ordered queue approach which can result in clients not
knowing whether their request will ever be served. To mitigate this issue, we
instead use a fixed size queue of size $q$. Whenever that queue is full, the
internal difficulty threshold is incremented by 1 and all requests after that
point must meet at least that internal difficulty threshold. If the internal
difficulty threshold doesn't increment for $q$ client requests in a row then the
internal difficulty threshold is decremented by 1. The external difficulty
threshold given to clients is 1 greater than $d$ except when $d = 0$. When
$d = 0$ the external difficulty is also $0$.

### Mitigation Steps when Under Load

A local adversary could unduly burden the network by sending large messages to
various random locations around the address space without the following
countermeasures.

The overall countermeasure is to have nodes declare the maximum size of the
messages they expect to receive is. If the total size of all messages exceeds
the expected size declared then the node will evict (forget about) the
oldest-seen messages, essentially keeping a FIFO ring buffer of messages.

At the sky node layer, when connecting into a sky node one will establish a
"lease" at that sky node. If the sky node is malicious then that will leak how
long you intend to stay at that sky node. However, a good sky node will simply
block all lookup queries which do not correspond to a node's lease.

Secondly, earth nodes will only keep track of nodes which have preemptively
announced their lease. Leases are generally kept track of, until their
expiration date, by both sky and earth nodes.

#

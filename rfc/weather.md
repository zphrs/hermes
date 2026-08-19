# Weather 

How Earth Nodes Avoid Deceitful Sky Nodes.

## Problem

Properly functioning sky nodes have no natural incentive to exist other than 
contributing to the network. Meanwhile byzantine sky nodes can enable DoS of
the network especially if earth nodes don't know that a specific sky node
is lying. To mitigate this, we have earth nodes gossip about sky nodes. We call
such gossip about sky nodes a "weather report" because it's earthly observations
of the sky.

## Weather Report

The weather report consists of time intervals with a maximum of 32 second time
precision.

```rust
struct Interval {
    start: u64,
    duration: Option<u64>,
    log2_timestep: u8, // at least 5
    sky_node: SkyNode,
}
```

The formulas for constructing a new interval is:

```rust

pub fn new(start: Instant, end: Option<Instant>) -> Interval {
    let start = start.duration_since(UNIX_EPOCH).as_secs() + 32;
    let Some(end) = end else {
        return Interval {
          start: start / 32,
          duration: None,
        };
    }
    let end = end.duration_since(UNIX_EPOCH).as_secs() - 32;
    let duration = end - start;

    let log2_timestep = max(floor(log2(duration - 64)), log2(32))
    Interval {
        start: start / 2^log2_timestep,
        duration: duration / 2^log2_timestep,
        log2_timestep,
    }
}
```

Intervals are collected into a report like so:

```rust
struct Report {
    intervals: MaxVecLen<100, Interval>,
    author: EarthNode,
    signature: EarthNodeSignature,
}
```

If there are more than 100 intervals in the last 24 hours then the oldest
reports are omitted.

Earth nodes keep track of both weather reports and all of the results they have
gotten from various sky nodes. This lets earth nodes compare the results
returned by sky nodes with the intervals reported by other earth nodes. This
allows earth nodes to realize when a sky node might have omitted reporting an
earth node that was connected to it and significantly penalize its reputation
for doing so. If 24 hours have passed since an earth node has received a result
from a sky node and there have been no inconsistencies found with intervals from
other earth nodes in that time then the sky node's reputation in increased by
one and then is divided by two. If instead there have been conflicts reported
with the sky node then it is increased by `max(1 - 0.05 *
sum(conflict_reporters.map(|reporter| reporter.reputation()), 0)` and then
divided by two. The reporter's reputation is the reputation an earth
node has via the [trust system](./trust-system.md). This means earth nodes that
have earned trust of their neighbors to store data when they're offline also 
gain the trust of their neighbors to report when they have been online. This
can ultimately slow down the network if an earth node acts benevolently when it
comes to storing data but then acts maliciously by misreporting which sky nodes
they were connected to at a given time. Thus we rely on the difficulty of having
both a high trust score and being nearest to an arbitrary queried point 
(the queried address) simultaneously without having a majority of earth nodes 
controlled overall.

In other words we assume malicious actors will control the minority of earth
nodes because otherwise the network trivially is rendered useless (and thus
would require switching to a more federated permissioned system anyway) and we
use that core assumption that most earth nodes share the same goal of having the
system be maximally usable in order to hold sky nodes accountable to the earth
nodes. In other words, with great power (in terms of DoS) comes significant 
responsibility to the majority of good earth nodes.

## Initial Reputations of Sky Nodes

By default the reputation of a sky node is 0 and thus does not count towards
hitting the minimum number needed. To mitigate the issue of all sky nodes having
a reputation of 0 for a earth node new to the network, the bootstrap nodes
specified in the earth node configuration are initially trusted as having
successfully served one request with zero errors (reputation of 0.5). 
Alternatively, the source who provided the list of bootstrap nodes can also 
specify their trust levels for all of the bootstrap nodes provided. Since all
earth nodes interact with all sky nodes, any source of bootstrapping sky nodes
will have experience with all sky nodes and know how reputable all of them are.


## How Sky Node Reputation is Used

Then, when connecting to sky nodes and when querying sky nodes cumulative 
reputation is used in order to choose how much redundancy should be used. By
default, this value is configured to be 3, meaning that typically 4-6 nodes will
be connected to, depending on past experiences with the nearest sky nodes.

Reputation tracking informs earth nodes how many sky nodes they must connect to
simultaneously in order to have a reasonable chance at being included
cumulatively in responses to queries by other earth nodes and how many sky nodes
they must query when making requests to connect to earth nodes.

## When Earth Nodes Exchange/Broadcast Reports

Earth nodes share reports whenever they connect directly, as part of their
regular subscription broadcast, and when they send messages to friends who they
have received a message from in the last day.

The third case, when sending messages to friends, sends all known reports that
were within the hour of the most recent message(s) sent by the friend (known by
the message timestamp). This way the earth node who messaged you (who has the
exact message timestamp) will be able to figure out whether any sky nodes
omitted earth nodes that were nearer than the ones reported. This is necessary,
even though all earth nodes will use to all sky nodes for local neighborhood
replication because a sky node can distinguish between local neighborhood
traffic and traffic from nodes outside of a neighborhood. Thus, receiving friends'
weather reports provides a way to locally audit that sky nodes continue to be
honest about who is connected to them.

Intervals are deleted if they are older than 24 hours. This keeps the storage
overhead constant per earth node because each earth node will connect to a
similar number of sky nodes and interact with a roughly constant number of
neighbors.

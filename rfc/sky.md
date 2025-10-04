# The Sky Layer

The Sky facilitates peer-to-peer connections between users. Clients message the
service with their IP address, port, and target user ID. The Sky then attempts
to initialize a connection to that specific user and discloses whether the user
is online.

Sky nodes are typically in cloud computing environments with static IP addresses
and high uptime but costly egress fees.

## Privacy Considerations

Constantly disclosing whether or not a specific end user is online to the sky is
a confidentiality concern. This is mitigated through the ratcheting of virtual
addresses every R hours, as described above. This way old friends merely need to
ratchet their friend's id meanwhile sky nodes have no way to ratchet the user's
id because the ratcheting algorithm requires a secret piece of data.

Since different users connect to different sky servers based on their current
id, unless a user gets incredibly unlucky with their ratcheted ids, consistently
ending up with the same node operator, the times they are online will be
anonymized and the list of devices they specifically are messaging will be
untrackable, assuming that the majority of sky nodes do not store and publish
the IP addresses of those who connect to the sky.

This can further be anonymized by having the user randomly report that they are
online even when inactive while the program is running in the background through
background wakeups of the application. That said, randomization of sky operators
will likely be good enough at anonymizing user activity provided there are
enough sky operators online.

Furthermore, since sky operators merely facilitate the connection of two online
users and do not see the actual messages sent, sky operators really only have
information about a specific id at a specific time, especially since user ids
are consistently ratcheting forward.

## Address Reset Handling

This works until you have a former friend who a user wants to remove from their
friend list and furthermore prevent that friend from being able to track their
online status. When a friend is removed from a friend list, a user can reliably
broadcast a total change of address to all of their friends. To prevent losing
friends from a total address change, a user can repeatedly broadcast messages to
their old friends who have not acknowledged their address change yet. This
broadcasting only needs to be repeated at around the interval at which messages
expire in the earth cache.

## Architecture: Sky Node DHT Distribution

The sky is a DHT where nodes are distributed within the DHT based on their IP
address to deter sky nodes from intentionally following a specific earth node
around the DHT while ensuring load distribution across all sky nodes. Since
members of the sky explicitly are supposed to have open networks which actively
invite users to connect to them directly in order to facilitate connections, it
makes sense that their public IP address would be the way to accomplish load
balancing.

## IP-Based Temporary Address Tracking

Sky nodes need to know your current IP address, your current user ID, when
you're online, and whether you're on a metered connection, in order to connect
you to other earth nodes. If most sky nodes are controlled by a single actor
then that single actor will possibly be able to track a specific earth node
based on the coupling of an IP address connecting to their previous sky node
while simultaneously connecting to their new sky node. However, since sky nodes
running the original software do not forward along an earth node's IP address to
other sky nodes, in order to exfiltrate a user's IP address via the sky, an
actor would have to control the majority of the nodes in the sky network in
order to by chance control both the sky node hosting the old temporary address
and the new temporary address and have the user log on during the first H hours
of the R rotate window, which while likely, isn't guaranteed.

This means that tracking an individual user's temporary addresses over time is
difficult because it is both expensive (requires controlling a majority of the
sky network, which requires many IP addresses) and error prone (requires getting
lucky in controlling the user's new sky node and the user happening to check
their inbox during the first H hours of the R rotate window, neither of which
are guaranteed).

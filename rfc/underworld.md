# The Underworld Layer

While the underworld depends on the earth and sky for message traversal, the
rest of hermes does not rely on the underworld, rather users rely on the
underworld to use the sky and the earth in a privacy-preserving manner. Thus,
this document is more of an outline of how one might use hermes, rather than the
one and only way to use the rest of the system to send messages.

The Underworld is responsible for performing reliable delivery of messages and
ratcheting virtual addresses of the node and of the node's friends, every R
hours. Notably, virtual addresses are rotated at irregular intervals to ensure
that the entire network doesn't change inboxes all at once which would result in
a ton of network churn all at once as all earth nodes shuffle around
simultaneously. Thus, the time when the first address was originally generated
serves as an anchor time to rotate the key next.

Underworld nodes are clients' end machines (metered or unmetered) that know the
client's encryption keys and contact list, making them the only nodes that can
determine where, when, and how to send messages to their intended recipients.

## Handling Group Chats

Each group has its own ratcheting virtual address consisting of the usual
(root_addr, secret, creation_date, ratcheting_frequency) tuple which allows
derivation of future virtual addresses. This address can also change anytime a
member leaves the group. This ensures that the address is only known by the
smallest list of users who need to know it.

Group chats use some sort of Decentralized Group Key Agreement protocol (DGKA)
in order to agree on a shared key (Examples include Key Agreement for
Decentralized Secure Group Messaging with Strong Security Guarantees, Keyhive,
and CausalKEM). All users in the chat are able to message each other by simply
sending a direct message encrypted and addressed to a specific user in the chat
(peer-to-peer encryption keys are determined through an asymmetric key exchange
which takes place in the public chat). Essentially, when a member leaves the
group, members will use the old chat in order to establish necessary asymmetric
encrypted peer-to-peer chats with other members who are still in the group. If
the necessary peer-to-peer chat already exists

## Responsibilities

-   Dating messages (timestamping)
-   Signing messages
-   Encrypting messages
-   Reliably delivering messages (rebroadcasting until acknowledgement)
-   Ratcheting virtual addresses every R hours
-   Generating multiple inboxes for full groups

## Distribution

The underworld will be kept solely as a library with an FFI, which can then be
called through any language which supports FFIs.

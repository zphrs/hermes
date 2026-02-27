# Trust System

## assumptions

Only care about app-level concerns; i.e. not worried about nodes who want to
watch the world burn, instead worried about selfish nodes and dishonest nodes.

Comprised of the following assumptions:

-   The “good” state is common. I.e. nodes are only rarely overwhelmed during
    actively malicious attacks which are discouraged with PoW; we assume 
    post-scarcity messaging abilities.
-   Proof of work coupled with a theory of abundance ensures that nodes who want 
    to watch the world burn are mitigated and that nodes which are overwhelmed 
    will be up front about how they are overwhelmed.
-   Nodes cannot trust one another’s “goodness” ratings. Only first hand
    experience counts.
-   No money/monetary system/linearized log. Only game theory
-   Targeted attacks against a specific Node are considered out of scope
-   Nodes behave selfishly at worst and at best perform neutrally; not selfishly
    nor selflessly.

## how it works

Message headers contain at least the following when leaving the original sender:
`(addressed_to, sent_at_hour, msg_hash, recip_pub_key, sender_signed(message hash), 
recip_signed(recipient address, message hash))`.

- `addressed_to` determines where in the DHT it lands
- `sent_at_hour` determines when it is safe to discard the message 
  abs(SystemTime::now() / (1 hour) - sent_at_hour) > 2
- ``

At each hop along the way the peer storing and forwarding (the forwarder) will 
append to the header the following: 
(Forwarder's public key, hash of header, encrypted header hash)

The forwarder will also wrap the message contents itself in another layer of
cryptography.

This anonymizes the recipient’s address without burdening the recipient with
downloading all messages within R, instead they only download the headers of all
messages within R, a task they should already be doing in order to be a
cooperative node to avoid receiving messages they already have. With the sender's
header it is possible to figure out which friend sent the message without 
revealing any information about the sender. From there it should be possible to 
decrypt the encrypted recipient address by using their public key with the sender's
private key.

Secondly, if a recipient successfully gets a message delivered (verified with
sender’s signature after decryption) they increment the trust of the node which
replied with the message addressed to them and of anyone else who replied to the
recipient’s request for headers who also had the message addressed to them.

Lastly, if a node replies to a request for headers but is then unable to
actually “follow through” when requesting the actual data of the message their
reputation is decremented.

Reputation is used to prioritize API requests, including for message metadata
requests and for raw message data requests. Bandwidth is split proportionally to
reputation.

Issues left over:

1. Temporarily holding onto a message isn’t rewarded with any reputation, even
   if eventually it is eventually delivered.
    - Could be solved somehow, maybe?
2. Senders must be trusted to randomize the recipient’s address, otherwise nodes
   could game the system by selectively sending along headers which match the
   address of the requester which would allow them to gain reputation without
   having to help the network replicate messages. That said, senders should gain
   no direct advantage from not anonymizing the message and receivers would only
   gain nominal advantage because they might be able to only fetch headers of
   the addresses nearest to them. This can be mitigated by always replying with
   the K nearest headers in a randomized order.

### why it works

Because nodes don’t know whether a headers request will be used for message
delivery or not, they are incentivized to send out all of their message headers
since delivering any of the headers’ messages might result in a bump to their
reputation.

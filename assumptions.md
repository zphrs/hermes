# Assumptions

If any items on the following list do not continue to be true then an old binary
of Hermes will stop working. My hope is that these assumptions are safe to
assume for at least the next century so that applications that care about
working for the next century can safely depend on Hermes.

- OS-level support for interacting with:
    - file system
    - UDP socket
    - random number generation
    - a heap
- An underlying network that can send and receive IP packets with the latency
  and throughput characteristics of the current internet (worst case ~5% packet
  loss and a few seconds of latency)
- An abundance of network bandwidth and disk space for end user devices
- An abundance of more technical people willing to run sky nodes that require a
  generally accessible IP address to help facilitate connections between
  end-user devices
- Continued existence of secure cryptographic signatures, asymmetric
  encryption, and symmetric encryption
- net neutrality/isps not rate limiting traffic that looks like peer-to-peer 
  traffic
    - this assumption is legally mandated with net neutrality in some regions,
      including California
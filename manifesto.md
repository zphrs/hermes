# Manifesto

Hermes is designed to be usable for at least a century to provide reliable
delayed delivery of end-to-end encrypted messages between unreliable peers of
the internet (think mobile phones with sporadic uptime in the worst case). We
strive to assume little about the environment Hermes will run within, aside from
aspects of the internet that has existed for around a half-century. That doesn't
mean the implementations cannot bundle new technology within its binary, but
that the binary must not rely on the external environment for anything that has
not continued to hold true for the past half-century. These assumptions include:

- OS-level Internet Protocol support (either IPv4 or IPv6) (1980)
- OS-level UDP protocol support (1980)
- an underlying network that can send and receive UDP packets with the
  reliability and latency of today's internet (worst case ~5% packet loss and a
  few seconds of latency)
- secure cryptographic primitives like signatures and encryption
    - RSA was first publicly described in 1977

There are more assumptions not listed here. For a full list see
[assumptions.md](./assumptions.md).

Ultimately as long as these assumptions hold, that have held for the past
half-century, applications that rely on Hermes should continue to work.

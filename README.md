# Hermes

A peer-to-peer delay-tolerant network (DTN).

Essentially it defines a set of protocols to enable devices to send messages to
one another, even if two communicating devices are never online at the same time
and even if both are behind firewalls and network address translators (NATs).

## Structure

Hermes consists of two node types; earth nodes and sky nodes. Earth nodes are
the nodes handling the forwarding of message data with one another while Sky
nodes play matchmaker, helping earth nodes find other earth nodes and
facilitating the establishment of direct connections between them with NAT
traversal.

## License

Licensed under [GPL-v3](./LICENSE).

## Credits

Hermes directly relies on the following libraries:

- [minicbor](https://github.com/twittner/minicbor/tree/develop) and [minicbor_io](https://github.com/twittner/minicbor/tree/develop/minicbor-io) (licensed under the [Blue Oak Model License](https://blueoakcouncil.org/license/1.0.0)).
- Licensed under the MIT License:
    - tokio crates:
        - [tokio](https://tokio.rs/)
        - [tracing](https://github.com/tokio-rs/tracing)
        - [bytes](https://github.com/tokio-rs/bytes)
- Dual licensed under MIT and Apache 2.0:
    - [arrayvec](https://github.com/bluss/arrayvec)
    - [thiserror](https://github.com/dtolnay/thiserror)
    - futures crates:
        - [futures](https://github.com/rust-lang/futures-rs)
        - [futures-io](https://github.com/rust-lang/futures-rs/tree/main/futures-io)
    - [rand](https://github.com/rust-random/rand)
    - [syn](https://github.com/dtolnay/syn)
    - 
    - [quote](https://github.com/dtolnay/quote)
    - [proc_macro2](https://github.com/dtolnay/proc-macro2)
    - [proc-macro-crate](https://github.com/bkchr/proc-macro-crate)
    - [quinn](https://github.com/quinn-rs/quinn)
    - [crypto-bigint](https://github.com/RustCrypto/crypto-bigint)
    - [sha2](https://github.com/RustCrypto/hashes)

For development purposes (just to build and test, not included in the final binary) Hermes relies on several more libraries. These are specified under the `[dev-dependencies]` in each crate's Cargo.toml file. All dependencies of the `dens` (Discrete-Event Network Simulator) crate are purely used for testing.

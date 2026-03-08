# maxlen-derive

A procedural macro for deriving the `MaxLen` trait, which is used to determine the maximum CBOR-encoded length of a type.

## Documentation

For complete documentation with examples, see the [crate documentation](https://docs.rs/maxlen-derive):

```bash
cargo doc --open -p maxlen-derive
```

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
maxlen-derive = { path = "../maxlen-derive" }
minicbor = { version = "2.1.3", features = ["derive"] }
```

Use the derive macro:

```rust
use maxlen_derive::MaxLen;
use minicbor::{Encode, Decode, CborLen};

#[derive(Encode, Decode, CborLen, MaxLen)]
struct MyStruct {
    #[n(0)]
    value: u32,
}

#[derive(Encode, Decode, CborLen, MaxLen)]
#[cbor(flat)]
enum MyEnum {
    #[n(0)]
    Variant1,
    #[n(1)]
    Variant2(#[n(0)] u32),
}
```

## Migration Guide

See [MIGRATION.md](./MIGRATION.md) for a guide on replacing manual `MaxLen` implementations with the derive macro.

## Examples

Run the example:

```bash
cargo run --example usage -p maxlen-derive
```

## Tests

Run the test suite:

```bash
cargo test -p maxlen-derive
```

## License

See LICENSE file in the workspace root.

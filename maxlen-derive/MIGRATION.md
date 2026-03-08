# Migration Guide: Using MaxLen Derive

This guide shows how to replace manual `MaxLen` implementations with the `#[derive(MaxLen)]` macro.

## Setup

First, ensure `maxlen-derive` is added to your dependencies in `Cargo.toml`:

```toml
[dependencies]
maxlen-derive = { version = "0.1.0", path = "../maxlen-derive" }
```

Then import it in your schema module:

```rust
use maxlen_derive::MaxLen;
```

## Migration Examples

### Example 1: Simple Struct with Named Fields

**Before:**
```rust
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct Register {
    #[n(0)]
    pub neighbors: Option<[Option<SkyNode>; 20]>,
}

impl MaxLen for Register {
    fn biggest_instantiation() -> Self {
        Register {
            neighbors: Some(array::from_fn(|_| Some(SkyNode::biggest_instantiation()))),
        }
    }
}
```

**After:**
```rust
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct Register {
    #[n(0)]
    pub neighbors: Option<[Option<SkyNode>; 20]>,
}
```

### Example 2: Tuple Struct

**Before:**
```rust
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct NearbyEarthNodes(#[n(0)] [Option<EarthNode>; 20]);

impl MaxLen for NearbyEarthNodes {
    fn biggest_instantiation() -> Self {
        Self(array::from_fn(|_| Some(EarthNode::biggest_instantiation())))
    }
}
```

**After:**
```rust
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct NearbyEarthNodes(#[n(0)] [Option<EarthNode>; 20]);
```

### Example 3: Enum with Multiple Variants

**Before:**
```rust
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cbor(flat)]
pub enum EarthToSkyRequestValue {
    #[n(0)]
    Register,
    #[n(1)]
    EarthNodesNear(#[n(0)] EarthId),
    #[n(2)]
    ConnectTo(#[n(0)] EarthNode),
}

impl MaxLen for EarthToSkyRequestValue {
    fn biggest_instantiation() -> Self {
        let variants = [
            Self::Register,
            Self::EarthNodesNear(MaxLen::biggest_instantiation()),
            Self::ConnectTo(MaxLen::biggest_instantiation()),
        ];
        variants
            .into_iter()
            .max_by_key(|v| minicbor::len(v))
            .unwrap()
    }
}
```

**After:**
```rust
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
#[cbor(flat)]
pub enum EarthToSkyRequestValue {
    #[n(0)]
    Register,
    #[n(1)]
    EarthNodesNear(#[n(0)] EarthId),
    #[n(2)]
    ConnectTo(#[n(0)] EarthNode),
}
```

### Example 4: Complex Enum with Tagged Variants

**Before:**
```rust
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cbor(flat)]
pub enum RequestType {
    #[n(0)]
    Ping(#[n(0)] ping::Request),
    #[n(1)]
    FindSkyNode(#[n(0)] FindSkyNodeRequest),
    #[n(2)]
    FromEarth(#[n(0)] EarthToSkyRequest),
    #[n(3)]
    EarthNodesNear(#[n(0)] EarthId),
}

impl MaxLen for RequestType {
    fn biggest_instantiation() -> Self {
        let variants = [
            Self::Ping(MaxLen::biggest_instantiation()),
            Self::FindSkyNode(MaxLen::biggest_instantiation()),
            Self::FromEarth(MaxLen::biggest_instantiation()),
            Self::EarthNodesNear(MaxLen::biggest_instantiation()),
        ];
        variants
            .into_iter()
            .max_by_key(|v| minicbor::len(v))
            .unwrap()
    }
}
```

**After:**
```rust
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
#[cbor(flat)]
pub enum RequestType {
    #[n(0)]
    Ping(#[n(0)] ping::Request),
    #[n(1)]
    FindSkyNode(#[n(0)] FindSkyNodeRequest),
    #[n(2)]
    FromEarth(#[n(0)] EarthToSkyRequest),
    #[n(3)]
    EarthNodesNear(#[n(0)] EarthId),
}
```

## Migration Checklist

For each type you want to migrate:

1. ✅ Ensure all field types already implement `MaxLen`
2. ✅ Add `MaxLen` to the derive attribute list
3. ✅ Remove the manual `impl MaxLen for Type` block
4. ✅ Keep all `minicbor` derive attributes and attributes like `#[cbor(flat)]`
5. ✅ Run `cargo test` to verify the behavior is preserved

## Important Notes

### Field Type Requirements

All field types must implement `MaxLen`. The derive macro will call `MaxLen::biggest_instantiation()` on each field. If a field type doesn't implement `MaxLen`, you'll get a compilation error.

### Enum Variant Selection

For enums, the derive macro:
1. Creates all possible variants with `biggest_instantiation()` for all fields
2. Encodes each variant to CBOR
3. Returns the variant with the largest encoded size

This is the same algorithm as the manual implementations.

### Generics

The derive macro supports generics, but generic type parameters must also have the `MaxLen` bound if they're used in fields. Example:

```rust
#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, MaxLen)]
pub struct Container<T: MaxLen> {
    #[n(0)]
    value: T,
}
```

### When NOT to Use the Derive Macro

The derive macro generates standard implementations. If you need custom logic (e.g., choosing a specific variant for reasons other than maximum size), keep the manual implementation.

## Benefits

Using the derive macro provides:

- **Less boilerplate**: No need to write repetitive implementations
- **Consistency**: All types use the same algorithm
- **Maintainability**: Changes to fields automatically update the implementation
- **Reduced errors**: No risk of forgetting to update the implementation when adding fields

## Testing

After migration, verify that the maximum lengths are calculated correctly:

```rust
#[test]
fn test_max_len() {
    let instance = MyType::biggest_instantiation();
    let encoded = minicbor::to_vec(&instance).unwrap();
    assert_eq!(encoded.len(), MyType::max_len());
}
```

## Rollout Strategy

Recommended migration approach:

1. Start with leaf types (types with no dependencies)
2. Move up to types that depend on those leaf types
3. Test incrementally after each batch of migrations
4. Keep the existing tests to ensure behavior is preserved

## Support

If you encounter issues or edge cases not covered by the derive macro, please:
1. Check if a manual implementation is more appropriate
2. Open an issue with details about your use case
3. Consider contributing enhancements to the derive macro
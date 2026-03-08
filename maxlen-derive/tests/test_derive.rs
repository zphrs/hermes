use maxlen::MaxLen;
use minicbor::{CborLen, Decode, Encode};

// Test basic struct with named fields
#[derive(Encode, Decode, CborLen, MaxLen)]
struct SimpleStruct {
    #[n(0)]
    value: u32,
    #[n(1)]
    flag: bool,
}

// Test tuple struct
#[derive(Encode, Decode, CborLen, MaxLen)]
struct TupleStruct(#[n(0)] u32, #[n(1)] bool);

// Test unit struct
#[derive(Encode, Decode, CborLen, MaxLen)]
struct UnitStruct;

// Test enum with various field types
#[derive(Encode, Decode, CborLen, MaxLen)]
#[cbor(flat)]
enum TestEnum {
    #[n(0)]
    Unit,
    #[n(1)]
    Single(#[n(0)] u32),
    #[n(2)]
    Tuple(#[n(0)] u32, #[n(1)] bool),
    #[n(3)]
    Struct {
        #[n(0)]
        value: u32,
        #[n(1)]
        flag: bool,
    },
}

// Test nested structures
#[derive(Encode, Decode, CborLen, MaxLen)]
struct Nested {
    #[n(0)]
    inner: SimpleStruct,
}

// Test Option fields (Option impl provided by maxlen crate)
#[derive(Encode, Decode, CborLen, MaxLen)]
struct WithOption {
    #[n(0)]
    maybe: Option<u32>,
}

// Test array fields (array impl provided by maxlen crate)
#[derive(Encode, Decode, CborLen, MaxLen)]
struct WithArray {
    #[n(0)]
    items: [u32; 5],
}

#[test]
fn test_simple_struct() {
    let instance = SimpleStruct::biggest_instantiation();
    assert_eq!(instance.value, u32::MAX);
    assert_eq!(instance.flag, true);
}

#[test]
fn test_tuple_struct() {
    let instance = TupleStruct::biggest_instantiation();
    assert_eq!(instance.0, u32::MAX);
    assert_eq!(instance.1, true);
}

#[test]
fn test_unit_struct() {
    let _instance = UnitStruct::biggest_instantiation();
}

#[test]
fn test_enum() {
    let instance = TestEnum::biggest_instantiation();
    // The enum should pick the variant with the largest CBOR encoding
    let len = minicbor::len(&instance);

    // Verify it's one of the variants
    match instance {
        TestEnum::Unit | TestEnum::Single(_) | TestEnum::Tuple(_, _) | TestEnum::Struct { .. } => {
            // Test passes if it compiles and creates a valid variant
        }
    }

    assert!(len > 0);
}

#[test]
fn test_nested() {
    let instance = Nested::biggest_instantiation();
    assert_eq!(instance.inner.value, u32::MAX);
}

#[test]
fn test_with_option() {
    let instance = WithOption::biggest_instantiation();
    assert!(instance.maybe.is_some());
    assert_eq!(instance.maybe.unwrap(), u32::MAX);
}

#[test]
fn test_with_array() {
    let instance = WithArray::biggest_instantiation();
    for item in instance.items {
        assert_eq!(item, u32::MAX);
    }
}

#[test]
fn test_max_len_init() {
    let len = SimpleStruct::max_len_init();
    let instance = SimpleStruct::biggest_instantiation();
    let actual_len = minicbor::len(&instance);
    assert_eq!(len, actual_len);
}

#[test]
fn test_max_len_caching() {
    let len1 = SimpleStruct::max_len();
    let len2 = SimpleStruct::max_len();
    assert_eq!(len1, len2);
    assert!(len1 > 0);
}

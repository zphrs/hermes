//! A procedural macro for deriving the `MaxLen` trait, which is used to determine the maximum
//! CBOR-encoded length of a type.
//!
//! # Overview
//!
//! This crate provides a derive macro for the [`MaxLen`](maxlen::MaxLen) trait from the `maxlen` crate.
//! The trait creates instances with the maximum possible CBOR encoding size. This is useful for:
//!
//! - Pre-allocating buffers for CBOR encoding
//! - Determining maximum packet sizes
//! - Protocol design and validation
//!
//! # Setup
//!
//! Add both `maxlen` and `maxlen-derive` to your dependencies:
//!
//! ```toml
//! [dependencies]
//! maxlen = "0.1.0"
//! maxlen-derive = "0.1.0"
//! minicbor = { version = "2.1.3", features = ["derive"] }
//! ```
//!
//! # The MaxLen Trait
//!
//! The `MaxLen` trait from the `maxlen` crate provides:
//!
//! - `biggest_instantiation()` - Creates an instance with maximum CBOR size
//! - `max_len_init()` - Calculates the maximum CBOR-encoded length
//! - `max_len()` - Returns cached maximum length
//!
//! The derive macro only implements `biggest_instantiation()`. The other methods have default
//! implementations that use it.
//!
//! # Deriving for Structs
//!
//! The derive macro works with named, unnamed (tuple), and unit structs:
//!
//! ```
//! use maxlen::MaxLen;
//! use maxlen_derive::MaxLen;
//! use minicbor::{Encode, Decode, CborLen};
//!
//! #[derive(Encode, Decode, CborLen, MaxLen)]
//! struct MyStruct {
//!     #[n(0)]
//!     value: u32,
//!     #[n(1)]
//!     flag: bool,
//! }
//!
//! // Tuple struct
//! #[derive(Encode, Decode, CborLen, MaxLen)]
//! struct TupleStruct(#[n(0)] u32, #[n(1)] bool);
//!
//! // Unit struct
//! #[derive(Encode, Decode, CborLen, MaxLen)]
//! struct UnitStruct;
//!
//! // Test the generated implementations
//! let my_struct = MyStruct::biggest_instantiation();
//! assert_eq!(my_struct.value, u32::MAX);
//! assert_eq!(my_struct.flag, true);
//!
//! let tuple = TupleStruct::biggest_instantiation();
//! assert_eq!(tuple.0, u32::MAX);
//! assert_eq!(tuple.1, true);
//!
//! let _unit = UnitStruct::biggest_instantiation();
//! ```
//!
//! For structs, the derive macro calls `MaxLen::biggest_instantiation()` on each field.
//!
//! # Deriving for Enums
//!
//! The derive macro also supports enums:
//!
//! ```
//! use maxlen::MaxLen;
//! use minicbor::{Encode, Decode, CborLen};
//!
//! #[derive(Encode, Decode, CborLen, MaxLen)]
//! #[cbor(flat)]
//! enum MyEnum {
//!     #[n(0)]
//!     Unit,
//!     #[n(1)]
//!     Single(#[n(0)] u32),
//!     #[n(2)]
//!     Tuple(#[n(0)] u32, #[n(1)] bool),
//!     #[n(3)]
//!     Struct {
//!         #[n(0)]
//!         value: u32,
//!         #[n(1)]
//!         flag: bool,
//!     },
//! }
//!
//! // The derive macro picks the variant with the largest CBOR encoding
//! let instance = MyEnum::biggest_instantiation();
//! let len = minicbor::len(&instance);
//! assert!(len > 0);
//! ```
//!
//! For enums, the derive macro:
//! 1. Creates an instance of each variant with `biggest_instantiation()` for all fields
//! 2. Encodes each variant using `minicbor::len()`
//! 3. Returns the variant with the maximum encoded length
//!
//! # Working with Options and Arrays
//!
//! The macro works seamlessly with `Option` and array types when they implement `MaxLen`:
//!
//! ```
//! use maxlen::MaxLen;
//! use minicbor::{Encode, Decode, CborLen};
//!
//! // Option and array impls are provided by the maxlen crate
//!
//! #[derive(Encode, Decode, CborLen, MaxLen)]
//! struct WithOption {
//!     #[n(0)]
//!     maybe: Option<u32>,
//! }
//!
//! #[derive(Encode, Decode, CborLen, MaxLen)]
//! struct WithArray {
//!     #[n(0)]
//!     items: [u32; 5],
//! }
//!
//! let with_option = WithOption::biggest_instantiation();
//! assert!(with_option.maybe.is_some());
//! assert_eq!(with_option.maybe.unwrap(), u32::MAX);
//!
//! let with_array = WithArray::biggest_instantiation();
//! for item in with_array.items {
//!     assert_eq!(item, u32::MAX);
//! }
//! ```
//!
//! # Nested Structures
//!
//! The derive macro handles nested types automatically:
//!
//! ```
//! use maxlen::MaxLen;
//! use minicbor::{Encode, Decode, CborLen};
//!
//! #[derive(Encode, Decode, CborLen, MaxLen)]
//! struct User {
//!     #[n(0)]
//!     id: u64,
//!     #[n(1)]
//!     active: bool,
//! }
//!
//! #[derive(Encode, Decode, CborLen, MaxLen)]
//! struct Request {
//!     #[n(0)]
//!     user: User,
//! }
//!
//! let request = Request::biggest_instantiation();
//! assert_eq!(request.user.id, u64::MAX);
//! assert_eq!(request.user.active, true);
//! ```
//!
//! # Real-World Example
//!
//! Here's a complete example similar to the schema crate usage:
//!
//! ```
//! use maxlen::MaxLen;
//! use minicbor::{Encode, Decode, CborLen};
//!
//! // Note: Option and array impls are provided by the maxlen crate
//!
//! // Define domain types
//! #[derive(Encode, Decode, CborLen, DeriveMaxLen)]
//! struct Node {
//!     #[n(0)]
//!     id: u64,
//! }
//!
//! #[derive(Encode, Decode, CborLen, MaxLen)]
//! struct RegisterResponse {
//!     #[n(0)]
//!     neighbors: Option<[Option<Node>; 20]>,
//! }
//!
//! #[derive(Encode, Decode, CborLen, MaxLen)]
//! #[cbor(flat)]
//! enum RequestType {
//!     #[n(0)]
//!     Ping,
//!     #[n(1)]
//!     GetNode(#[n(0)] u64),
//!     #[n(2)]
//!     Register(#[n(0)] Node),
//! }
//!
//! // Calculate maximum sizes
//! let register_max = RegisterResponse::max_len_init();
//! let request_max = RequestType::max_len_init();
//!
//! assert!(register_max > 0);
//! assert!(request_max > 0);
//!
//! // The enum picks the largest variant
//! let request = RequestType::biggest_instantiation();
//! match request {
//!     RequestType::Ping |
//!     RequestType::GetNode(_) |
//!     RequestType::Register(_) => {
//!         // All variants are valid
//!     }
//! }
//! ```
//!
//! # Features
//!
//! ## Supported
//!
//! - ✅ Structs with named fields
//! - ✅ Structs with unnamed fields (tuple structs)
//! - ✅ Unit structs
//! - ✅ Enums with all variant types
//! - ✅ Generic types (with appropriate bounds)
//! - ✅ Nested types
//! - ✅ Works with all `minicbor` attributes (`#[cbor(flat)]`, `#[cbor(tag(n))]`, etc.)
//!
//! ## Not Supported
//!
//! - ❌ Unions (will panic at compile time)
//! - ❌ Custom logic for variant selection (use manual implementation)
//!
//! # Limitations
//!
//! The derive macro assumes:
//! 1. All field types implement `MaxLen`
//! 2. The "biggest" enum variant is the one with the maximum CBOR encoding size
//! 3. Standard CBOR encoding is used (via `minicbor::len`)

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

/// Derives the `MaxLen` trait for structs and enums.
///
/// This macro generates an implementation of `MaxLen::biggest_instantiation()` that:
/// - For structs: calls `biggest_instantiation()` on each field
/// - For enums: creates all variants, encodes them, and returns the largest
///
/// # Examples
///
/// See the crate-level documentation for comprehensive examples.
#[proc_macro_derive(MaxLen)]
pub fn derive_max_len(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let implementation = match &input.data {
        Data::Struct(data_struct) => {
            let field_inits = match &data_struct.fields {
                Fields::Named(fields) => {
                    let field_assignments = fields.named.iter().map(|field| {
                        let field_name = &field.ident;
                        quote! {
                            #field_name: MaxLen::biggest_instantiation()
                        }
                    });
                    quote! {
                        Self {
                            #(#field_assignments),*
                        }
                    }
                }
                Fields::Unnamed(fields) => {
                    let field_inits = fields.unnamed.iter().map(|_| {
                        quote! { MaxLen::biggest_instantiation() }
                    });
                    quote! {
                        Self(#(#field_inits),*)
                    }
                }
                Fields::Unit => {
                    quote! { Self }
                }
            };

            quote! {
                impl #impl_generics MaxLen for #name #ty_generics #where_clause {
                    fn biggest_instantiation() -> Self {
                        #field_inits
                    }
                }
            }
        }
        Data::Enum(data_enum) => {
            let variant_constructions = data_enum.variants.iter().map(|variant| {
                let variant_name = &variant.ident;
                match &variant.fields {
                    Fields::Named(fields) => {
                        let field_assignments = fields.named.iter().map(|field| {
                            let field_name = &field.ident;
                            quote! {
                                #field_name: MaxLen::biggest_instantiation()
                            }
                        });
                        quote! {
                            Self::#variant_name {
                                #(#field_assignments),*
                            }
                        }
                    }
                    Fields::Unnamed(fields) => {
                        let field_inits = fields.unnamed.iter().map(|_| {
                            quote! { MaxLen::biggest_instantiation() }
                        });
                        quote! {
                            Self::#variant_name(#(#field_inits),*)
                        }
                    }
                    Fields::Unit => {
                        quote! { Self::#variant_name }
                    }
                }
            });

            quote! {
                impl #impl_generics MaxLen for #name #ty_generics #where_clause {
                    fn biggest_instantiation() -> Self {
                        let variants = [
                            #(#variant_constructions),*
                        ];
                        variants
                            .into_iter()
                            .max_by_key(|v| minicbor::len(v))
                            .unwrap()
                    }
                }
            }
        }
        Data::Union(_) => {
            panic!("MaxLen cannot be derived for unions");
        }
    };

    implementation.into()
}

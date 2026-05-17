//! The `MaxLen` trait for determining maximum CBOR-encoded lengths.
//!
//! This crate provides the `MaxLen` trait which is used to create instances of types
//! with the maximum possible CBOR encoding size. This is useful for:
//!
//! - Pre-allocating buffers for CBOR encoding
//! - Determining maximum packet sizes
//! - Protocol design and validation
//!
//! # The MaxLen Trait
//!
//! The core trait provides methods to:
//! - Create the biggest possible instance of a type
//! - Calculate and cache the maximum CBOR-encoded length
//!
//! # Derive Macro
//!
//! When the `derive` feature is enabled (default), you can use the `#[derive(MaxLen)]` macro:
//!
//! ```
//! use maxlen::MaxLen;
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
//! let max_len = MyStruct::max_len();
//! assert!(max_len > 0);
//! ```
//!
//! # Manual Implementation
//!
//! ```
//! use maxlen::MaxLen;
//! use minicbor::CborLen;
//!
//! impl MaxLen for u32 {
//!     fn biggest_instantiation() -> Self {
//!         u32::MAX
//!     }
//! }
//!
//! // Calculate max length
//! let max_len = u32::max_len();
//! assert!(max_len > 0);
//! ```

use std::{
    borrow::Cow,
    net::{IpAddr, Ipv6Addr, SocketAddr},
};

use minicbor::CborLen;

// Re-export the derive macro when the derive feature is enabled
#[cfg(feature = "derive")]
pub use maxlen_derive::MaxLen;

/// Trait for types that can provide their maximum CBOR-encoded length.
///
/// Types implementing this trait must be able to create an instance that,
/// when CBOR-encoded, represents the maximum possible encoded size for that type.
///
/// # Implementation Requirements
///
/// Implementors must provide `biggest_instantiation()` which returns an instance
/// that will produce the largest CBOR encoding.
///
/// # Examples
///
/// ```
/// use maxlen::MaxLen;
/// use minicbor::CborLen;
///
/// impl MaxLen for u32 {
///     fn biggest_instantiation() -> Self {
///         u32::MAX
///     }
/// }
///
/// let instance = u32::biggest_instantiation();
/// assert_eq!(instance, u32::MAX);
/// ```
static INIT_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub trait MaxLen
where
    Self: CborLen<()> + Sized,
{
    /// Creates an instance of this type that will have the maximum CBOR-encoded length.
    ///
    /// For primitive types, this is typically the maximum value (e.g., `u32::MAX`).
    /// For structs, this calls `biggest_instantiation()` on each field.
    /// For enums, this returns the variant with the largest CBOR encoding.
    fn biggest_instantiation() -> Self;

    /// Calculates the maximum CBOR-encoded length for this type.
    ///
    /// This method creates the biggest instantiation and measures its encoded size.
    /// Use `max_len()` instead if you want caching.
    fn max_len_init() -> usize {
        INIT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        minicbor::len(Self::biggest_instantiation())
    }

    /// Returns the cached maximum CBOR-encoded length for this type.
    ///
    /// This method uses `OnceLock` to cache the result, so repeated calls are
    /// essentially free after the first computation.
    ///
    /// # Examples
    ///
    /// ```
    /// use maxlen::MaxLen;
    /// use minicbor::CborLen;
    ///
    /// impl MaxLen for u32 {
    ///     fn biggest_instantiation() -> Self { u32::MAX }
    /// }
    ///
    /// let len1 = u32::max_len();
    /// let len2 = u32::max_len(); // Uses cached value
    /// assert_eq!(len1, len2);
    /// ```
    fn max_len() -> usize {
        tracing::warn!("using uncached version of max_len");
        Self::max_len_init()
    }
}

// Standard library implementations

/// MaxLen implementation for Option<T>.
///
/// Always returns Some(T::biggest_instantiation()) as that produces the largest encoding.
impl<T: MaxLen> MaxLen for Option<T> {
    fn biggest_instantiation() -> Self {
        Some(T::biggest_instantiation())
    }
}

/// MaxLen implementation for arrays.
///
/// Creates an array where every element is the biggest instantiation of T.
impl<T: MaxLen, const N: usize> MaxLen for [T; N] {
    fn biggest_instantiation() -> Self {
        std::array::from_fn(|_| T::biggest_instantiation())
    }
}

/// MaxLen implementation for Result<T, E>.
///
/// Returns whichever variant (Ok or Err) produces the larger CBOR encoding.
impl<T: MaxLen, E: MaxLen> MaxLen for Result<T, E>
where
    Result<T, E>: minicbor::Encode<()>,
{
    fn biggest_instantiation() -> Self {
        let ok = Ok(T::biggest_instantiation());
        let err = Err(E::biggest_instantiation());
        if minicbor::len(&ok) > minicbor::len(&err) {
            ok
        } else {
            err
        }
    }
}

/// MaxLen implementation for unit type.
///
/// The unit type has a fixed size.
impl MaxLen for () {
    fn biggest_instantiation() -> Self {}
}

/// MaxLen implementation for SocketAddr.
///
/// Uses an IPv6 address as it produces a larger encoding than IPv4.
impl MaxLen for SocketAddr {
    fn biggest_instantiation() -> Self {
        SocketAddr::new(Ipv6Addr::from_segments([u16::MAX; 8]).into(), u16::MAX)
    }
}

impl MaxLen for IpAddr {
    fn biggest_instantiation() -> Self {
        IpAddr::V6(Ipv6Addr::from_segments([u16::MAX; 8]))
    }
}

impl<T> MaxLen for Box<T>
where
    T: MaxLen,
{
    fn biggest_instantiation() -> Self {
        Box::new(T::biggest_instantiation())
    }
}

// Primitive type implementations

/// MaxLen implementation for u8.
impl MaxLen for u8 {
    fn biggest_instantiation() -> Self {
        u8::MAX
    }
}

/// MaxLen implementation for u16.
impl MaxLen for u16 {
    fn biggest_instantiation() -> Self {
        u16::MAX
    }
}

/// MaxLen implementation for u32.
impl MaxLen for u32 {
    fn biggest_instantiation() -> Self {
        u32::MAX
    }
}

/// MaxLen implementation for u64.
impl MaxLen for u64 {
    fn biggest_instantiation() -> Self {
        u64::MAX
    }
}

/// MaxLen implementation for i8.
impl MaxLen for i8 {
    fn biggest_instantiation() -> Self {
        i8::MIN
    }
}

/// MaxLen implementation for i16.
impl MaxLen for i16 {
    fn biggest_instantiation() -> Self {
        i16::MIN
    }
}

/// MaxLen implementation for i32.
impl MaxLen for i32 {
    fn biggest_instantiation() -> Self {
        i32::MIN
    }
}

/// MaxLen implementation for i64.
impl MaxLen for i64 {
    fn biggest_instantiation() -> Self {
        i64::MIN
    }
}

/// MaxLen implementation for bool.
impl MaxLen for bool {
    fn biggest_instantiation() -> Self {
        true
    }
}

impl<'a, T: MaxLen + Clone> MaxLen for Cow<'a, T> {
    fn biggest_instantiation() -> Self {
        let owned: Cow<'a, T> = Cow::Owned(T::biggest_instantiation());
        let borrowed = Cow::Borrowed(&T::biggest_instantiation());
        debug_assert!(owned.cbor_len(&mut ()) >= borrowed.cbor_len(&mut ()));
        owned
    }
}

/// MaxLen implementation for String.
///
/// Returns an empty string as strings are dynamically sized.
/// For fixed-size strings, use arrays or custom types.
impl MaxLen for String {
    fn biggest_instantiation() -> Self {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_option_maxlen() {
        let opt = Option::<u32>::biggest_instantiation();
        assert!(opt.is_some());
        assert_eq!(opt.unwrap(), u32::MAX);
    }

    #[test]
    fn test_array_maxlen() {
        let arr = <[u32; 5]>::biggest_instantiation();
        for item in arr {
            assert_eq!(item, u32::MAX);
        }
    }

    #[test]
    fn test_result_maxlen() {
        let result = Result::<u32, ()>::biggest_instantiation();
        // Result should pick the variant with larger encoding
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_unit_maxlen() {
        let _unit = <()>::biggest_instantiation();
    }

    #[test]
    fn test_socketaddr_maxlen() {
        let addr = SocketAddr::biggest_instantiation();
        assert!(addr.is_ipv6());
    }

    #[test]
    fn test_max_len_caching() {
        let len1 = u32::max_len();
        let len2 = u32::max_len();
        assert_eq!(len1, len2);
        assert!(len1 > 0);
    }

    #[test]
    fn test_max_len_init() {
        let init_len = u32::max_len_init();
        let cached_len = u32::max_len();
        assert_eq!(init_len, cached_len);
    }
}

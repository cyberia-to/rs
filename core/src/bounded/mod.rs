//! Bounded collections with compile-time capacity limits.
//!
//! - [`BoundedVec<T, N>`] — fixed-capacity vector with inline storage
//! - [`BoundedMap<K, V, N>`] — sorted array map with binary search
//! - [`ArrayString<N>`] — fixed-capacity UTF-8 string

mod vec;
mod map;
mod string;

pub use vec::BoundedVec;
pub use map::BoundedMap;
pub use string::ArrayString;

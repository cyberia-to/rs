//! Rs standard library — types, bounded collections, fixed-point arithmetic,
//! arena allocator, and bounded channels.
//!
//! This crate is `#![no_std]` by default. Enable the `std` feature for
//! standard library integration and test support.

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

#[cfg(any(feature = "std", test))]
extern crate alloc;

pub mod core_types;
pub mod fixed_point;
pub mod bounded;
pub mod arena;
pub mod channel;
pub mod runtime;

// Re-export proc-macros when the `macros` feature is enabled.
// This follows the serde pattern: users depend on `rs-lang = { features = ["macros"] }`
// and get both the runtime types and proc-macros through a single crate.
#[cfg(feature = "macros")]
pub use rs_lang_macros::*;

// Re-export core types at crate root for ergonomic access.
pub use core_types::{
    Address, BufMut, CanonicalSerialize, Cell, CellMetadata, FunctionSignature,
    HealthStatus, MigrateFrom, Particle, StepReset, Timeout,
};

/// Prelude module — import everything with `use rs_lang::prelude::*;`
pub mod prelude {
    pub use crate::core_types::{
        Address, BufMut, CanonicalSerialize, Cell, CellMetadata, FunctionSignature,
        HealthStatus, MigrateFrom, Particle, StepReset, Timeout,
    };
    pub use crate::fixed_point::FixedPoint;
    pub use crate::bounded::{ArrayString, BoundedMap, BoundedVec};
    pub use crate::arena::Arena;
    pub use crate::channel::{bounded_channel, Full, Receiver, Sender};
}

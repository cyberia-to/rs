//! darwin-sys — minimum Darwin ABI surface for the pure-Rust std macOS backend.
//!
//! Covers every libSystem entry point that std needs: filesystem, process,
//! threading, synchronisation, time, environment, and randomness.
//!
//! Design principles (mirrors honeycrisp):
//! - `#![no_std]` — usable as a substrate for std itself.
//! - Zero external crate dependencies.
//! - Raw FFI in `ffi::*` (unsafe); safe wrappers at the module root.
//! - Errors are `OsError(i32)` wrapping the raw errno value.

#![no_std]
#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]
#![cfg(target_os = "macos")]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod error;
pub mod ffi;
pub mod fs;
pub mod process;
pub mod thread;
pub mod sync;
pub mod time;
pub mod env;
pub mod rand;

pub use error::{OsError, Result};
pub use ffi::types::*;

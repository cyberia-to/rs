//! Raw FFI layer — `extern "C"` blocks that bind libSystem.dylib.
//! All items are `unsafe`.  Callers are responsible for upholding the
//! documented preconditions of each function.

pub mod types;
pub mod fs;
pub mod process;
pub mod thread;
pub mod sync;
pub mod misc;
pub mod signal;

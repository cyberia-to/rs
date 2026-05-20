//! Raw synchronisation FFI — ulock (macOS 10.12+) and memory barriers.
//!
//! `__ulock_wait` and `__ulock_wake` are private libSystem entry points used
//! by std's Mutex implementation on macOS.  They are stable in practice
//! (Swift and Rust std both depend on them) despite the `__` prefix.

use crate::ffi::types::*;

#[link(name = "System")]
extern "C" {
    /// Atomically compare *addr with value; if equal, block the calling thread.
    /// Returns 0 on wake, -1 with errno EINTR on signal, -1 with errno ETIMEDOUT
    /// on timeout (timeout_us == 0 → wait forever).
    pub fn __ulock_wait(
        operation:  u32,
        addr:       *mut c_void,
        value:      u64,
        timeout_us: u32,
    ) -> c_int;

    /// Wake one (or all, with ULF_WAKE_ALL) threads waiting on addr.
    /// Returns the number of threads woken, or -1 on error.
    pub fn __ulock_wake(
        operation:  u32,
        addr:       *mut c_void,
        wake_value: u64,
    ) -> c_int;
}

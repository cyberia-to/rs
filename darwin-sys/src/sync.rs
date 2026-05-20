//! Safe synchronisation wrappers: ulock-based futex-like primitives.
//!
//! `ULock` wraps the private `__ulock_wait`/`__ulock_wake` pair used by
//! macOS's std Mutex — identical to what Rust's libstd does on Darwin.

use crate::ffi::sync as raw;
use crate::ffi::types::*;
use crate::error::{OsError, Result};

// UL_COMPARE_AND_WAIT, UL_UNFAIR_LOCK, ULF_WAKE_ALL, ULF_NO_ERRNO
// are re-exported from ffi::types via the glob import above.

// ── Raw wrappers ─────────────────────────────────────────────────────────

/// Block the calling thread until `*addr != value`.
///
/// `timeout_us == 0` means wait forever.
/// Returns `Ok(())` on wake, `Err(EINTR)` on signal, `Err(ETIMEDOUT)` on timeout.
#[inline]
pub fn ulock_wait(operation: u32, addr: *mut u32, value: u32, timeout_us: u32) -> Result<()> {
    let r = unsafe { raw::__ulock_wait(operation, addr as *mut c_void, value as u64, timeout_us) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

/// Wake one (or all with `ULF_WAKE_ALL`) threads waiting on `addr`.
/// Returns the number of threads woken.
#[inline]
pub fn ulock_wake(operation: u32, addr: *mut u32) -> Result<u32> {
    let r = unsafe { raw::__ulock_wake(operation, addr as *mut c_void, 0) };
    if r < 0 { Err(OsError::last()) } else { Ok(r as u32) }
}

// ── ULock: unfair lock using ulock primitives ────────────────────────────

/// A lightweight unfair spinlock backed by `__ulock_wait`/`__ulock_wake`.
///
/// This mirrors exactly what libstd's `Mutex` uses on macOS.
/// State: 0 = unlocked, 1 = locked, 2 = locked+waiters.
pub struct ULock {
    state: core::sync::atomic::AtomicU32,
}

impl ULock {
    pub const fn new() -> Self {
        Self { state: core::sync::atomic::AtomicU32::new(0) }
    }

    pub fn lock(&self) {
        use core::sync::atomic::Ordering::*;
        if self.state.compare_exchange(0, 1, Acquire, Relaxed).is_ok() {
            return;
        }
        self.lock_slow();
    }

    fn lock_slow(&self) {
        use core::sync::atomic::Ordering::*;
        loop {
            let state = self.state.swap(2, Acquire);
            if state == 0 { return; }
            let addr = self.state.as_ptr();
            let _ = ulock_wait(UL_UNFAIR_LOCK, addr, 2, 0);
        }
    }

    pub fn unlock(&self) {
        use core::sync::atomic::Ordering::*;
        let prev = self.state.swap(0, Release);
        if prev == 2 {
            let _ = ulock_wake(UL_UNFAIR_LOCK, self.state.as_ptr());
        }
    }

    pub fn try_lock(&self) -> bool {
        use core::sync::atomic::Ordering::*;
        self.state.compare_exchange(0, 1, Acquire, Relaxed).is_ok()
    }
}

/// RAII guard returned by [`ULock::lock`] — not exposed directly since
/// `ULock::lock` doesn't return a guard (matches std's raw mutex API).
/// Use `ULockGuard::acquire` as the ergonomic entry point.
pub struct ULockGuard<'a>(&'a ULock);

impl<'a> ULockGuard<'a> {
    pub fn acquire(lock: &'a ULock) -> Self {
        lock.lock();
        Self(lock)
    }
}

impl Drop for ULockGuard<'_> {
    fn drop(&mut self) { self.0.unlock(); }
}

// ── Futex-style wait/wake on arbitrary u32 addresses ─────────────────────

/// Wait until `*addr != expected`, or `timeout_us` microseconds elapse.
///
/// Wraps `__ulock_wait(UL_COMPARE_AND_WAIT, ...)`.
#[inline]
pub fn futex_wait(addr: *mut u32, expected: u32, timeout_us: u32) -> Result<()> {
    ulock_wait(UL_COMPARE_AND_WAIT, addr, expected, timeout_us)
}

/// Wake one thread waiting on `addr`.
#[inline]
pub fn futex_wake(addr: *mut u32) -> Result<u32> {
    ulock_wake(UL_COMPARE_AND_WAIT, addr)
}

/// Wake all threads waiting on `addr`.
#[inline]
pub fn futex_wake_all(addr: *mut u32) -> Result<u32> {
    ulock_wake(UL_COMPARE_AND_WAIT | ULF_WAKE_ALL, addr)
}

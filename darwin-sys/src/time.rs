//! Safe time wrappers: monotonic clock, realtime clock, sleep.

use crate::ffi::misc as raw;
use crate::ffi::types::*;
use crate::error::{OsError, Result};

// ── Clock IDs ───────────────────────────────────────────────────────────

pub use crate::ffi::types::{
    CLOCK_REALTIME,
    CLOCK_MONOTONIC,
    CLOCK_MONOTONIC_RAW,
    CLOCK_PROCESS_CPUTIME_ID,
    CLOCK_THREAD_CPUTIME_ID,
};

// ── Monotonic time ───────────────────────────────────────────────────────

/// Read a clock.  Returns `(seconds, nanoseconds)`.
pub fn clock_gettime(clk: clockid_t) -> Result<(i64, i64)> {
    let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
    let r = unsafe { raw::clock_gettime(clk, &mut ts) };
    if r < 0 { Err(OsError::last()) } else { Ok((ts.tv_sec as i64, ts.tv_nsec as i64)) }
}

/// Nanoseconds since an arbitrary monotonic reference point.
#[inline]
pub fn monotonic_ns() -> Result<u64> {
    let (s, ns) = clock_gettime(CLOCK_MONOTONIC)?;
    Ok(s as u64 * 1_000_000_000 + ns as u64)
}

/// Nanoseconds of CPU time used by the current thread.
#[inline]
pub fn thread_cpu_ns() -> Result<u64> {
    let (s, ns) = clock_gettime(CLOCK_THREAD_CPUTIME_ID)?;
    Ok(s as u64 * 1_000_000_000 + ns as u64)
}

/// Current wall-clock time as (seconds since Unix epoch, nanoseconds).
#[inline]
pub fn realtime() -> Result<(i64, i64)> {
    clock_gettime(CLOCK_REALTIME)
}

/// Wall-clock seconds since Unix epoch (coarse).
#[inline]
pub fn realtime_secs() -> Result<i64> {
    clock_gettime(CLOCK_REALTIME).map(|(s, _)| s)
}

// ── Sleep ────────────────────────────────────────────────────────────────

/// Sleep for at least `ns` nanoseconds, restarting on EINTR.
pub fn sleep_ns(ns: u64) -> Result<()> {
    let mut req = Timespec {
        tv_sec:  (ns / 1_000_000_000) as i64,
        tv_nsec: (ns % 1_000_000_000) as i64,
    };
    loop {
        let mut rem = Timespec { tv_sec: 0, tv_nsec: 0 };
        let r = unsafe { raw::nanosleep(&req, &mut rem) };
        if r == 0 { return Ok(()); }
        let e = OsError::last();
        if e.is_interrupted() {
            req = rem;
            continue;
        }
        return Err(e);
    }
}

/// Sleep for at least `ms` milliseconds.
#[inline]
pub fn sleep_ms(ms: u64) -> Result<()> { sleep_ns(ms * 1_000_000) }

// ── gettimeofday ─────────────────────────────────────────────────────────

/// Read `gettimeofday`.  Returns `(tv_sec, tv_usec)`.
pub fn gettimeofday() -> Result<(i64, i32)> {
    let mut tv = Timeval::default();
    let r = unsafe { raw::gettimeofday(&mut tv, core::ptr::null_mut()) };
    if r < 0 { Err(OsError::last()) } else { Ok((tv.tv_sec as i64, tv.tv_usec as i32)) }
}

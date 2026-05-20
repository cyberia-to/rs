//! Raw FFI for miscellaneous syscalls: time, randomness, environment,
//! working directory, and system information.

use crate::ffi::types::*;

#[link(name = "System")]
extern "C" {
    // ── Time ────────────────────────────────────────────────────────
    pub fn clock_gettime(clk_id: clockid_t, tp: *mut Timespec) -> c_int;
    pub fn clock_getres(clk_id: clockid_t, tp: *mut Timespec)  -> c_int;
    pub fn gettimeofday(tv: *mut Timeval, tz: *mut c_void)      -> c_int;
    pub fn nanosleep(rqtp: *const Timespec, rmtp: *mut Timespec) -> c_int;

    // ── Randomness ──────────────────────────────────────────────────
    /// Fill buf with entropy from the OS.  Max buf size is 256 bytes per call.
    /// Returns 0 on success, -1 on error (only EINVAL/EFAULT possible).
    pub fn getentropy(buf: *mut c_void, buflen: size_t) -> c_int;

    // ── Environment ─────────────────────────────────────────────────
    /// Returns a pointer to the NUL-terminated value, or NULL if not set.
    /// The returned pointer is valid until the next call to setenv/unsetenv.
    pub fn getenv(name: *const c_char) -> *const c_char;
    pub fn setenv(name: *const c_char, value: *const c_char, overwrite: c_int) -> c_int;
    pub fn unsetenv(name: *const c_char) -> c_int;
    /// NULL-terminated array of "KEY=VALUE\0" strings; do not modify.
    pub static mut environ: *const *const c_char;

    // ── Working directory ────────────────────────────────────────────
    pub fn getcwd(buf: *mut c_char, size: size_t) -> *mut c_char;
    pub fn chdir(path: *const c_char) -> c_int;

    // ── System info ──────────────────────────────────────────────────
    pub fn sysctl(
        name:    *const c_int,
        namelen: c_uint,
        oldp:    *mut c_void,
        oldlenp: *mut size_t,
        newp:    *const c_void,
        newlen:  size_t,
    ) -> c_int;

    pub fn sysctlbyname(
        name:    *const c_char,
        oldp:    *mut c_void,
        oldlenp: *mut size_t,
        newp:    *const c_void,
        newlen:  size_t,
    ) -> c_int;

    pub fn uname(name: *mut Utsname) -> c_int;
    pub fn gethostname(name: *mut c_char, namelen: size_t) -> c_int;

    // ── Capabilities / resource limits ──────────────────────────────
    pub fn getrlimit(resource: c_int, rlp: *mut Rlimit) -> c_int;
    pub fn setrlimit(resource: c_int, rlp: *const Rlimit) -> c_int;
}

// ── Utsname ────────────────────────────────────────────────────────────

/// macOS `struct utsname` — five fixed-length char arrays of 256 bytes each.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Utsname {
    pub sysname:  [u8; 256],
    pub nodename: [u8; 256],
    pub release:  [u8; 256],
    pub version:  [u8; 256],
    pub machine:  [u8; 256],
}

// ── Rlimit ─────────────────────────────────────────────────────────────

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct Rlimit {
    pub rlim_cur: u64,
    pub rlim_max: u64,
}

pub const RLIM_INFINITY:  u64 = u64::MAX;
pub const RLIMIT_CPU:     c_int = 0;
pub const RLIMIT_FSIZE:   c_int = 1;
pub const RLIMIT_DATA:    c_int = 2;
pub const RLIMIT_STACK:   c_int = 3;
pub const RLIMIT_CORE:    c_int = 4;
pub const RLIMIT_AS:      c_int = 5;
pub const RLIMIT_RSS:     c_int = 5;
pub const RLIMIT_MEMLOCK: c_int = 6;
pub const RLIMIT_NPROC:   c_int = 7;
pub const RLIMIT_NOFILE:  c_int = 8;

// ── sysctl MIB names ───────────────────────────────────────────────────

pub const CTL_KERN:       c_int = 1;
pub const CTL_HW:         c_int = 6;
pub const KERN_ARGMAX:    c_int = 8;
pub const KERN_PROC:      c_int = 14;
pub const HW_NCPU:        c_int = 3;
pub const HW_PHYSMEM:     c_int = 5;
pub const HW_PAGESIZE:    c_int = 7;

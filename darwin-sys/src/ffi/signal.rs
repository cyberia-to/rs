//! Raw signal FFI — signal, sigaction, sigprocmask, kill.

use crate::ffi::types::*;

/// Handler pointer encoded as a raw integer.
/// `SIG_DFL` = 0, `SIG_IGN` = 1, anything else is a real `unsafe extern "C" fn(c_int)`.
pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;
/// Error return from `signal()`.
pub const SIG_ERR: usize = usize::MAX;

/// macOS arm64 `struct sigaction` — 16 bytes.
///
/// Layout:
///   0  void (*sa_handler)(int) — 8 bytes (fn ptr or SIG_DFL/SIG_IGN)
///   8  sigset_t sa_mask        — 4 bytes
///  12  int      sa_flags       — 4 bytes
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Sigaction {
    pub sa_handler: usize,
    pub sa_mask:    sigset_t,
    pub sa_flags:   c_int,
}
const _SIGACTION_SIZE: () = assert!(core::mem::size_of::<Sigaction>() == 16);

// ── SA_* flags ──────────────────────────────────────────────────────────

pub const SA_ONSTACK:   c_int = 0x0001;
pub const SA_RESTART:   c_int = 0x0002;
pub const SA_RESETHAND: c_int = 0x0004;
pub const SA_NOCLDSTOP: c_int = 0x0008;
pub const SA_NODEFER:   c_int = 0x0010;
pub const SA_NOCLDWAIT: c_int = 0x0020;
pub const SA_SIGINFO:   c_int = 0x0040;

// ── sigprocmask `how` ───────────────────────────────────────────────────

pub const SIG_BLOCK:   c_int = 1;
pub const SIG_UNBLOCK: c_int = 2;
pub const SIG_SETMASK: c_int = 3;

// ── Raw bindings ────────────────────────────────────────────────────────

#[link(name = "System")]
extern "C" {
    /// Install a signal handler; returns the previous handler (or `SIG_ERR` on error).
    pub fn signal(signum: c_int, handler: usize) -> usize;

    /// Examine or change the action for a signal.
    pub fn sigaction(signum: c_int, act: *const Sigaction, oldact: *mut Sigaction) -> c_int;

    /// Modify the calling thread's signal mask.
    pub fn sigprocmask(how: c_int, set: *const sigset_t, oldset: *mut sigset_t) -> c_int;

    pub fn sigemptyset(set: *mut sigset_t) -> c_int;
    pub fn sigfillset(set: *mut sigset_t) -> c_int;
    pub fn sigaddset(set: *mut sigset_t, signo: c_int) -> c_int;
    pub fn sigdelset(set: *mut sigset_t, signo: c_int) -> c_int;
    pub fn sigismember(set: *const sigset_t, signo: c_int) -> c_int;

    /// Send signal `sig` to process `pid`.
    pub fn kill(pid: pid_t, sig: c_int) -> c_int;

    /// Send signal `sig` to the calling thread.
    pub fn raise(sig: c_int) -> c_int;
}

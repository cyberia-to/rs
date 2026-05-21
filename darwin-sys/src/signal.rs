//! Safe signal wrappers.
//!
//! Provides `signal()`, `sigaction()`, `sigprocmask()`, `kill()`, and `raise()`
//! with `OsError` conversion on failure.

use crate::ffi::signal as raw;
use crate::ffi::types::*;
use crate::error::{OsError, Result};

pub use raw::{
    Sigaction,
    SIG_DFL, SIG_IGN,
    SA_ONSTACK, SA_RESTART, SA_RESETHAND, SA_NOCLDSTOP,
    SA_NODEFER, SA_NOCLDWAIT, SA_SIGINFO,
    SIG_BLOCK, SIG_UNBLOCK, SIG_SETMASK,
};

/// Install a handler for `signum`.
///
/// `handler` is `SIG_DFL`, `SIG_IGN`, or an `unsafe extern "C" fn(c_int)` cast to `usize`.
/// Returns the previous handler value, or an error.
pub fn signal(signum: c_int, handler: usize) -> Result<usize> {
    let prev = unsafe { raw::signal(signum, handler) };
    if prev == raw::SIG_ERR { Err(OsError::last()) } else { Ok(prev) }
}

/// Examine or change the action for `signum`.  Returns the previous action.
pub fn sigaction(signum: c_int, act: &Sigaction) -> Result<Sigaction> {
    let mut old = Sigaction { sa_handler: SIG_DFL, sa_mask: 0, sa_flags: 0 };
    let r = unsafe { raw::sigaction(signum, act as *const _, &mut old as *mut _) };
    if r < 0 { Err(OsError::last()) } else { Ok(old) }
}

/// Build a `Sigaction` that ignores a signal (SA_RESTART set).
#[inline]
pub fn ignore_action() -> Sigaction {
    Sigaction { sa_handler: SIG_IGN, sa_mask: 0, sa_flags: SA_RESTART }
}

/// Build a `Sigaction` that restores the default disposition.
#[inline]
pub fn default_action() -> Sigaction {
    Sigaction { sa_handler: SIG_DFL, sa_mask: 0, sa_flags: 0 }
}

/// Build a `Sigaction` from a handler function pointer.
#[inline]
pub fn handler_action(f: unsafe extern "C" fn(c_int), flags: c_int) -> Sigaction {
    Sigaction { sa_handler: f as usize, sa_mask: 0, sa_flags: flags }
}

/// Modify the calling thread's signal mask.
///
/// `set` is `None` to query the current mask without changing it.
pub fn sigprocmask(how: c_int, set: Option<&sigset_t>) -> Result<sigset_t> {
    let mut old: sigset_t = 0;
    let p = match set {
        Some(s) => s as *const sigset_t,
        None    => core::ptr::null(),
    };
    let r = unsafe { raw::sigprocmask(how, p, &mut old) };
    if r < 0 { Err(OsError::last()) } else { Ok(old) }
}

/// Send signal `sig` to process `pid`.
pub fn kill(pid: pid_t, sig: c_int) -> Result<()> {
    let r = unsafe { raw::kill(pid, sig) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

/// Send signal `sig` to the calling thread.
pub fn raise(sig: c_int) -> Result<()> {
    let r = unsafe { raw::raise(sig) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

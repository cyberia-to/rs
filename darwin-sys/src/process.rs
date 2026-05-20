//! Safe process wrappers: identity, fork/exec, posix_spawn, wait, signals.

use crate::ffi::process as raw;
use crate::ffi::types::*;
use crate::error::{OsError, Result};

// ── Identity ────────────────────────────────────────────────────────────

#[inline] pub fn getpid()  -> pid_t { unsafe { raw::getpid() } }
#[inline] pub fn getppid() -> pid_t { unsafe { raw::getppid() } }
#[inline] pub fn getuid()  -> uid_t { unsafe { raw::getuid() } }
#[inline] pub fn getgid()  -> gid_t { unsafe { raw::getgid() } }
#[inline] pub fn geteuid() -> uid_t { unsafe { raw::geteuid() } }
#[inline] pub fn getegid() -> gid_t { unsafe { raw::getegid() } }

// ── Fork ────────────────────────────────────────────────────────────────

/// Fork the current process.
///
/// Returns `Ok(0)` in the child, `Ok(child_pid)` in the parent.
/// # Safety
/// After fork, only async-signal-safe functions may be called in the child
/// before exec.  Mutexes, allocators, and most runtime state are unsafe.
pub unsafe fn fork() -> Result<pid_t> {
    let pid = raw::fork();
    if pid < 0 { Err(OsError::last()) } else { Ok(pid) }
}

// ── Exec ────────────────────────────────────────────────────────────────

/// Execute `path` with the given argument and environment vectors.
///
/// On success this function does not return — the current process image
/// is replaced.  On error `Err(errno)` is returned.
///
/// `argv` and `envp` must be NULL-terminated slices of NUL-terminated
/// C string pointers.
pub unsafe fn execve(
    path: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> Result<core::convert::Infallible> {
    raw::execve(path, argv, envp);
    Err(OsError::last())
}

/// Like `execve` but searches PATH.
pub unsafe fn execvp(
    file: *const c_char,
    argv: *const *const c_char,
) -> Result<core::convert::Infallible> {
    raw::execvp(file, argv);
    Err(OsError::last())
}

// ── Wait ────────────────────────────────────────────────────────────────

/// Wait for `pid` to change state.  Returns (pid, raw_status).
pub fn waitpid(pid: pid_t, options: c_int) -> Result<(pid_t, c_int)> {
    let mut status: c_int = 0;
    let r = unsafe { raw::waitpid(pid, &mut status, options) };
    if r < 0 { Err(OsError::last()) } else { Ok((r, status)) }
}

/// Decoded exit status.
#[derive(Debug, Clone, Copy)]
pub enum ExitStatus {
    /// Process exited with the given code.
    Code(i32),
    /// Process was killed by the given signal.
    Signal(i32),
    /// Process is stopped (WIFSTOPPED).
    Stopped(i32),
}

impl ExitStatus {
    pub fn from_raw(status: c_int) -> Self {
        if raw::WIFEXITED(status) {
            Self::Code(raw::WEXITSTATUS(status))
        } else if raw::WIFSIGNALED(status) {
            Self::Signal(raw::WTERMSIG(status))
        } else {
            Self::Stopped(raw::WSTOPSIG(status))
        }
    }

    pub fn success(self) -> bool { matches!(self, Self::Code(0)) }
    pub fn code(self) -> Option<i32> { if let Self::Code(c) = self { Some(c) } else { None } }
}

// ── posix_spawn ─────────────────────────────────────────────────────────

/// RAII wrapper for `posix_spawn_file_actions_t`.
pub struct SpawnFileActions(PosixSpawnFileActionsT);

impl SpawnFileActions {
    pub fn new() -> Result<Self> {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        let r = unsafe { raw::posix_spawn_file_actions_init(&mut acts) };
        if r != 0 { Err(OsError(r)) } else { Ok(Self(acts)) }
    }

    pub fn add_open(&mut self, fildes: c_int, path: *const c_char, oflag: c_int, mode: mode_t) -> Result<()> {
        let r = unsafe { raw::posix_spawn_file_actions_addopen(&mut self.0, fildes, path, oflag, mode) };
        if r != 0 { Err(OsError(r)) } else { Ok(()) }
    }

    pub fn add_close(&mut self, fildes: c_int) -> Result<()> {
        let r = unsafe { raw::posix_spawn_file_actions_addclose(&mut self.0, fildes) };
        if r != 0 { Err(OsError(r)) } else { Ok(()) }
    }

    pub fn add_dup2(&mut self, fildes: c_int, newdes: c_int) -> Result<()> {
        let r = unsafe { raw::posix_spawn_file_actions_adddup2(&mut self.0, fildes, newdes) };
        if r != 0 { Err(OsError(r)) } else { Ok(()) }
    }
}

impl Drop for SpawnFileActions {
    fn drop(&mut self) { unsafe { raw::posix_spawn_file_actions_destroy(&mut self.0); } }
}

/// RAII wrapper for `posix_spawnattr_t`.
pub struct SpawnAttr(PosixSpanattrT);

impl SpawnAttr {
    pub fn new() -> Result<Self> {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpanattrT>() };
        let r = unsafe { raw::posix_spawnattr_init(&mut attr) };
        if r != 0 { Err(OsError(r)) } else { Ok(Self(attr)) }
    }

    pub fn set_flags(&mut self, flags: c_short) -> Result<()> {
        let r = unsafe { raw::posix_spawnattr_setflags(&mut self.0, flags) };
        if r != 0 { Err(OsError(r)) } else { Ok(()) }
    }
}

impl Drop for SpawnAttr {
    fn drop(&mut self) { unsafe { raw::posix_spawnattr_destroy(&mut self.0); } }
}

/// Spawn a new process using `posix_spawn`.  Returns the child PID.
pub fn spawn(
    path: *const c_char,
    file_actions: Option<&SpawnFileActions>,
    attr: Option<&SpawnAttr>,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> Result<pid_t> {
    let mut pid: pid_t = 0;
    let fa = file_actions.map_or(core::ptr::null(), |a| &a.0 as *const _);
    let at = attr.map_or(core::ptr::null(), |a| &a.0 as *const _);
    let r = unsafe { raw::posix_spawn(&mut pid, path, fa, at, argv, envp) };
    if r != 0 { Err(OsError(r)) } else { Ok(pid) }
}

// ── Signals ─────────────────────────────────────────────────────────────

pub fn kill(pid: pid_t, sig: c_int) -> Result<()> {
    let r = unsafe { raw::kill(pid, sig) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

// ── Exit ────────────────────────────────────────────────────────────────

#[inline]
pub fn exit(code: i32) -> ! { unsafe { raw::exit(code) } }

/// Exit without running atexit handlers or flushing stdio.
/// # Safety
/// Caller must ensure all important I/O has been flushed.
#[inline]
pub unsafe fn exit_raw(code: i32) -> ! { raw::_exit(code) }

//! Raw process FFI — fork, exec, wait, spawn, signals.

use crate::ffi::types::*;

#[link(name = "System")]
extern "C" {
    // ── Identity ────────────────────────────────────────────────────
    pub fn getpid()  -> pid_t;
    pub fn getppid() -> pid_t;
    pub fn getuid()  -> uid_t;
    pub fn getgid()  -> gid_t;
    pub fn geteuid() -> uid_t;
    pub fn getegid() -> gid_t;

    // ── Fork / exec ─────────────────────────────────────────────────
    pub fn fork() -> pid_t;

    /// Execute path with argv / envp.  argv and envp are NULL-terminated
    /// pointer arrays; each element is a NUL-terminated C string.
    pub fn execve(
        path:  *const c_char,
        argv:  *const *const c_char,
        envp:  *const *const c_char,
    ) -> c_int;

    /// Same as execve but searches PATH for the binary.
    pub fn execvp(file: *const c_char, argv: *const *const c_char) -> c_int;

    // ── Wait ────────────────────────────────────────────────────────
    pub fn waitpid(pid: pid_t, stat_loc: *mut c_int, options: c_int) -> pid_t;
    pub fn wait(stat_loc: *mut c_int) -> pid_t;

    // ── Exit ────────────────────────────────────────────────────────
    pub fn exit(status: c_int) -> !;
    pub fn _exit(status: c_int) -> !;
    pub fn abort() -> !;

    // ── Signals ─────────────────────────────────────────────────────
    pub fn kill(pid: pid_t, sig: c_int) -> c_int;
    pub fn raise(sig: c_int) -> c_int;

    // ── posix_spawn ─────────────────────────────────────────────────
    pub fn posix_spawn(
        pid:          *mut pid_t,
        path:         *const c_char,
        file_actions: *const PosixSpawnFileActionsT,
        attrp:        *const PosixSpanattrT,
        argv:         *const *const c_char,
        envp:         *const *const c_char,
    ) -> c_int;

    pub fn posix_spawnp(
        pid:          *mut pid_t,
        file:         *const c_char,
        file_actions: *const PosixSpawnFileActionsT,
        attrp:        *const PosixSpanattrT,
        argv:         *const *const c_char,
        envp:         *const *const c_char,
    ) -> c_int;

    // ── posix_spawn file actions ─────────────────────────────────────
    pub fn posix_spawn_file_actions_init(acts: *mut PosixSpawnFileActionsT) -> c_int;
    pub fn posix_spawn_file_actions_destroy(acts: *mut PosixSpawnFileActionsT) -> c_int;
    pub fn posix_spawn_file_actions_addopen(
        acts:  *mut PosixSpawnFileActionsT,
        fildes: c_int,
        path:  *const c_char,
        oflag: c_int,
        mode:  mode_t,
    ) -> c_int;
    pub fn posix_spawn_file_actions_addclose(
        acts:   *mut PosixSpawnFileActionsT,
        fildes: c_int,
    ) -> c_int;
    pub fn posix_spawn_file_actions_adddup2(
        acts:   *mut PosixSpawnFileActionsT,
        fildes: c_int,
        newdes: c_int,
    ) -> c_int;

    // ── posix_spawnattr ─────────────────────────────────────────────
    pub fn posix_spawnattr_init(attr: *mut PosixSpanattrT)    -> c_int;
    pub fn posix_spawnattr_destroy(attr: *mut PosixSpanattrT) -> c_int;
    pub fn posix_spawnattr_setflags(attr: *mut PosixSpanattrT, flags: c_short) -> c_int;
    pub fn posix_spawnattr_getflags(attr: *const PosixSpanattrT, flags: *mut c_short) -> c_int;
    pub fn posix_spawnattr_setsigmask(attr: *mut PosixSpanattrT, mask: *const sigset_t) -> c_int;
    pub fn posix_spawnattr_setsigdefault(attr: *mut PosixSpanattrT, mask: *const sigset_t) -> c_int;
}

// ── waitpid status macros ───────────────────────────────────────────────

/// True if the child exited normally.
#[inline] pub fn WIFEXITED(status: c_int)   -> bool { (status & 0x7f) == 0 }
/// Exit code of a normally-exited child.
#[inline] pub fn WEXITSTATUS(status: c_int) -> c_int { (status >> 8) & 0xff }
/// True if the child was terminated by a signal.
#[inline] pub fn WIFSIGNALED(status: c_int) -> bool { (((status & 0x7f) + 1) >> 1) > 0 }
/// Signal that terminated the child.
#[inline] pub fn WTERMSIG(status: c_int)    -> c_int { status & 0x7f }
/// True if the child is currently stopped.
#[inline] pub fn WIFSTOPPED(status: c_int)  -> bool { (status & 0xff) == 0x7f }
/// Signal that stopped the child.
#[inline] pub fn WSTOPSIG(status: c_int)    -> c_int { (status >> 8) & 0xff }

// ── posix_spawnattr flags ───────────────────────────────────────────────

pub const POSIX_SPAWN_RESETIDS:   i16 = 0x0001;
pub const POSIX_SPAWN_SETPGROUP:  i16 = 0x0002;
pub const POSIX_SPAWN_SETSIGDEF:  i16 = 0x0004;
pub const POSIX_SPAWN_SETSIGMASK: i16 = 0x0008;
pub const POSIX_SPAWN_SETEXEC:    i16 = 0x0040;
pub const POSIX_SPAWN_START_SUSPENDED: i16 = 0x0080;
pub const POSIX_SPAWN_CLOEXEC_DEFAULT: i16 = 0x4000;

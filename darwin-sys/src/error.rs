//! errno-based OS error type.

use core::fmt;
use crate::ffi::types::c_int;

// ── errno retrieval ─────────────────────────────────────────────────────

#[link(name = "System")]
extern "C" {
    /// Returns a pointer to the calling thread's errno variable.
    /// Never returns NULL.
    fn __error() -> *mut c_int;
}

/// Read the calling thread's errno.
#[inline]
pub fn errno() -> i32 {
    unsafe { *__error() }
}

/// Set the calling thread's errno.
#[inline]
pub fn set_errno(e: i32) {
    unsafe { *__error() = e; }
}

/// Clear errno (set to 0) before a call whose success indicator is
/// "errno unchanged" (e.g. `strtol`).
#[inline]
pub fn clear_errno() { set_errno(0); }

// ── OsError ────────────────────────────────────────────────────────────

/// A raw errno value from the OS.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct OsError(pub i32);

impl OsError {
    /// Capture errno from the last failing syscall.
    #[inline]
    pub fn last() -> Self { Self(errno()) }

    /// The raw errno value.
    #[inline]
    pub fn raw(self) -> i32 { self.0 }

    #[inline] pub fn is_interrupted(self)    -> bool { self.0 == EINTR }
    #[inline] pub fn is_would_block(self)    -> bool { self.0 == EAGAIN || self.0 == EWOULDBLOCK }
    #[inline] pub fn is_not_found(self)      -> bool { self.0 == ENOENT }
    #[inline] pub fn is_permission(self)     -> bool { self.0 == EACCES || self.0 == EPERM }
    #[inline] pub fn is_timed_out(self)      -> bool { self.0 == ETIMEDOUT }
    #[inline] pub fn is_already_exists(self) -> bool { self.0 == EEXIST }
}

impl fmt::Debug for OsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OsError({})", self.name())
    }
}

impl fmt::Display for OsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl OsError {
    fn name(self) -> &'static str {
        match self.0 {
            EPERM          => "EPERM: operation not permitted",
            ENOENT         => "ENOENT: no such file or directory",
            ESRCH          => "ESRCH: no such process",
            EINTR          => "EINTR: interrupted system call",
            EIO            => "EIO: I/O error",
            ENXIO          => "ENXIO: no such device or address",
            ENOEXEC        => "ENOEXEC: exec format error",
            EBADF          => "EBADF: bad file descriptor",
            ECHILD         => "ECHILD: no child processes",
            EDEADLK        => "EDEADLK: resource deadlock avoided",
            ENOMEM         => "ENOMEM: out of memory",
            EACCES         => "EACCES: permission denied",
            EFAULT         => "EFAULT: bad address",
            EBUSY          => "EBUSY: device busy",
            EEXIST         => "EEXIST: file exists",
            EXDEV          => "EXDEV: cross-device link",
            ENODEV         => "ENODEV: no such device",
            ENOTDIR        => "ENOTDIR: not a directory",
            EISDIR         => "EISDIR: is a directory",
            EINVAL         => "EINVAL: invalid argument",
            ENFILE         => "ENFILE: too many open files in system",
            EMFILE         => "EMFILE: too many open files",
            ENOTTY         => "ENOTTY: not a typewriter",
            EFBIG          => "EFBIG: file too large",
            ENOSPC         => "ENOSPC: no space left on device",
            ESPIPE         => "ESPIPE: illegal seek",
            EROFS          => "EROFS: read-only filesystem",
            EMLINK         => "EMLINK: too many links",
            EPIPE          => "EPIPE: broken pipe",
            ERANGE         => "ERANGE: result too large",
            EAGAIN         => "EAGAIN: resource temporarily unavailable",
            EINPROGRESS    => "EINPROGRESS: operation in progress",
            EALREADY       => "EALREADY: operation already in progress",
            ENOTSOCK       => "ENOTSOCK: socket operation on non-socket",
            EMSGSIZE       => "EMSGSIZE: message too long",
            ENOBUFS        => "ENOBUFS: no buffer space available",
            EISCONN        => "EISCONN: socket is already connected",
            ENOTCONN       => "ENOTCONN: socket is not connected",
            ETIMEDOUT      => "ETIMEDOUT: operation timed out",
            ECONNREFUSED   => "ECONNREFUSED: connection refused",
            ELOOP          => "ELOOP: too many levels of symbolic links",
            ENAMETOOLONG   => "ENAMETOOLONG: filename too long",
            ENOTEMPTY      => "ENOTEMPTY: directory not empty",
            EADDRINUSE     => "EADDRINUSE: address already in use",
            ECONNRESET     => "ECONNRESET: connection reset by peer",
            ENOTSUP        => "ENOTSUP: operation not supported",
            EOVERFLOW      => "EOVERFLOW: value too large to be stored",
            EILSEQ         => "EILSEQ: invalid or incomplete multibyte character",
            _              => "unknown error",
        }
    }
}

/// Convenience alias.
pub type Result<T> = core::result::Result<T, OsError>;

// ── errno constants (macOS) ─────────────────────────────────────────────

pub const EPERM:         i32 = 1;
pub const ENOENT:        i32 = 2;
pub const ESRCH:         i32 = 3;
pub const EINTR:         i32 = 4;
pub const EIO:           i32 = 5;
pub const ENXIO:         i32 = 6;
pub const ENOEXEC:       i32 = 8;
pub const EBADF:         i32 = 9;
pub const ECHILD:        i32 = 10;
pub const EDEADLK:       i32 = 11;
pub const ENOMEM:        i32 = 12;
pub const EACCES:        i32 = 13;
pub const EFAULT:        i32 = 14;
pub const EBUSY:         i32 = 16;
pub const EEXIST:        i32 = 17;
pub const EXDEV:         i32 = 18;
pub const ENODEV:        i32 = 19;
pub const ENOTDIR:       i32 = 20;
pub const EISDIR:        i32 = 21;
pub const EINVAL:        i32 = 22;
pub const ENFILE:        i32 = 23;
pub const EMFILE:        i32 = 24;
pub const ENOTTY:        i32 = 25;
pub const EFBIG:         i32 = 27;
pub const ENOSPC:        i32 = 28;
pub const ESPIPE:        i32 = 29;
pub const EROFS:         i32 = 30;
pub const EMLINK:        i32 = 31;
pub const EPIPE:         i32 = 32;
pub const ERANGE:        i32 = 34;
pub const EAGAIN:        i32 = 35;
pub const EWOULDBLOCK:   i32 = EAGAIN;
pub const EINPROGRESS:   i32 = 36;
pub const EALREADY:      i32 = 37;
pub const ENOTSOCK:      i32 = 38;
pub const EMSGSIZE:      i32 = 40;
pub const EADDRINUSE:    i32 = 48;
pub const ENOBUFS:       i32 = 55;
pub const EISCONN:       i32 = 56;
pub const ENOTCONN:      i32 = 57;
pub const ECONNRESET:    i32 = 54;
pub const ETIMEDOUT:     i32 = 60;
pub const ECONNREFUSED:  i32 = 61;
pub const ELOOP:         i32 = 62;
pub const ENAMETOOLONG:  i32 = 63;
pub const ENOTEMPTY:     i32 = 66;
pub const ENOTSUP:       i32 = 45;
pub const EOVERFLOW:     i32 = 84;
pub const EILSEQ:        i32 = 92;

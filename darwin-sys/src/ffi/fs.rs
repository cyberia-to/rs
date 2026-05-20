//! Raw filesystem FFI — open, close, read, write, seek, stat, directory ops.

use crate::ffi::types::*;

#[link(name = "System")]
extern "C" {
    // ── Open / close ────────────────────────────────────────────────
    pub fn open(path: *const c_char, oflag: c_int, ...) -> c_int;
    pub fn close(fd: c_int) -> c_int;
    pub fn dup(fd: c_int) -> c_int;
    pub fn dup2(fd: c_int, fd2: c_int) -> c_int;

    // ── Read / write ────────────────────────────────────────────────
    pub fn read(fd: c_int, buf: *mut c_void, nbyte: size_t) -> ssize_t;
    pub fn write(fd: c_int, buf: *const c_void, nbyte: size_t) -> ssize_t;
    pub fn pread(fd: c_int, buf: *mut c_void, nbyte: size_t, offset: off_t) -> ssize_t;
    pub fn pwrite(fd: c_int, buf: *const c_void, nbyte: size_t, offset: off_t) -> ssize_t;

    // ── Seek ────────────────────────────────────────────────────────
    pub fn lseek(fd: c_int, offset: off_t, whence: c_int) -> off_t;

    // ── File control ────────────────────────────────────────────────
    pub fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    pub fn fsync(fd: c_int) -> c_int;
    pub fn fdatasync(fd: c_int) -> c_int;
    pub fn ftruncate(fd: c_int, length: off_t) -> c_int;
    pub fn truncate(path: *const c_char, length: off_t) -> c_int;
    pub fn pipe(fds: *mut [c_int; 2]) -> c_int;

    // ── Metadata ────────────────────────────────────────────────────
    pub fn stat(path: *const c_char, buf: *mut Stat) -> c_int;
    pub fn lstat(path: *const c_char, buf: *mut Stat) -> c_int;
    pub fn fstat(fd: c_int, buf: *mut Stat) -> c_int;
    pub fn access(path: *const c_char, mode: c_int) -> c_int;

    // ── Directory ops ────────────────────────────────────────────────
    pub fn mkdir(path: *const c_char, mode: mode_t) -> c_int;
    pub fn rmdir(path: *const c_char) -> c_int;
    pub fn unlink(path: *const c_char) -> c_int;
    pub fn rename(old: *const c_char, new: *const c_char) -> c_int;
    pub fn link(existing: *const c_char, new: *const c_char) -> c_int;
    pub fn symlink(path1: *const c_char, path2: *const c_char) -> c_int;
    pub fn readlink(path: *const c_char, buf: *mut c_char, bufsize: size_t) -> ssize_t;

    // ── Directory iteration ──────────────────────────────────────────
    pub fn opendir(name: *const c_char) -> *mut c_void;
    pub fn readdir(dirp: *mut c_void) -> *mut Dirent;
    pub fn closedir(dirp: *mut c_void) -> c_int;

    // ── Memory-mapped I/O ────────────────────────────────────────────
    pub fn mmap(
        addr:   *mut c_void,
        length: size_t,
        prot:   c_int,
        flags:  c_int,
        fd:     c_int,
        offset: off_t,
    ) -> *mut c_void;
    pub fn munmap(addr: *mut c_void, length: size_t) -> c_int;
    pub fn mprotect(addr: *mut c_void, length: size_t, prot: c_int) -> c_int;
    pub fn madvise(addr: *mut c_void, length: size_t, advice: c_int) -> c_int;
}

// ── fcntl commands ──────────────────────────────────────────────────────

pub const F_DUPFD:       c_int = 0;
pub const F_GETFD:       c_int = 1;
pub const F_SETFD:       c_int = 2;
pub const F_GETFL:       c_int = 3;
pub const F_SETFL:       c_int = 4;
pub const F_SETLK:       c_int = 8;
pub const F_SETLKW:      c_int = 9;
pub const F_GETLK:       c_int = 7;
pub const F_DUPFD_CLOEXEC:c_int = 67;
pub const FD_CLOEXEC:    c_int = 1;

// ── access mode constants ───────────────────────────────────────────────

pub const F_OK: c_int = 0;
pub const X_OK: c_int = 1;
pub const W_OK: c_int = 2;
pub const R_OK: c_int = 4;

// ── madvise hints ───────────────────────────────────────────────────────

pub const MADV_NORMAL:     c_int = 0;
pub const MADV_RANDOM:     c_int = 1;
pub const MADV_SEQUENTIAL: c_int = 2;
pub const MADV_WILLNEED:   c_int = 3;
pub const MADV_DONTNEED:   c_int = 4;
pub const MADV_FREE:       c_int = 5;

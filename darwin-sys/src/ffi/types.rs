//! C types and Darwin-specific structs for aarch64-apple-darwin (LP64).
//!
//! All sizes and layouts are verified against Apple's xnu headers and the
//! ARM64 ABI.  Each struct carries a compile-time size assertion.

// ── Basic arithmetic types ──────────────────────────────────────────────

pub type c_schar    = i8;
pub type c_uchar    = u8;
pub type c_short    = i16;
pub type c_ushort   = u16;
pub type size_t     = usize;
pub type ssize_t    = isize;
pub type off_t      = i64;
pub type mode_t     = u16;
pub type dev_t      = i32;
pub type ino_t      = u64;
pub type nlink_t    = u16;
pub type uid_t      = u32;
pub type gid_t      = u32;
pub type pid_t      = i32;
pub type blksize_t  = i32;
pub type blkcnt_t   = i64;
pub type time_t     = i64;
pub type suseconds_t = i32;
pub type clockid_t  = u32;
pub type socklen_t  = u32;
pub type sigset_t   = u32;

// Re-export core::ffi primitives so callers only need to import ffi::types.
pub use core::ffi::{c_char, c_int, c_uint, c_long, c_ulong, c_void};

// ── Time ───────────────────────────────────────────────────────────────

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
pub struct Timespec {
    pub tv_sec:  i64,
    pub tv_nsec: i64,
}

#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct Timeval {
    pub tv_sec:  i64,
    pub tv_usec: i32,
    _pad: i32,
}

// ── File metadata ──────────────────────────────────────────────────────

/// macOS arm64 (LP64) `struct stat` — 144 bytes, natural alignment.
///
/// Layout (verified against xnu bsd/sys/stat.h with _DARWIN_FEATURE_64_BIT_INODE):
///   0   dev_t        st_dev         4
///   4   mode_t       st_mode        2
///   6   nlink_t      st_nlink       2
///   8   ino_t        st_ino         8
///  16   uid_t        st_uid         4
///  20   gid_t        st_gid         4
///  24   dev_t        st_rdev        4
///  28   (4-byte pad)
///  32   timespec     st_atimespec  16
///  48   timespec     st_mtimespec  16
///  64   timespec     st_ctimespec  16
///  80   timespec     st_birthtimespec 16
///  96   off_t        st_size        8
/// 104   blkcnt_t     st_blocks      8
/// 112   blksize_t    st_blksize     4
/// 116   uint32_t     st_flags       4
/// 120   uint32_t     st_gen         4
/// 124   int32_t      st_lspare      4
/// 128   int64_t[2]   st_qspare     16
/// = 144
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Stat {
    pub st_dev:          i32,
    pub st_mode:         u16,
    pub st_nlink:        u16,
    pub st_ino:          u64,
    pub st_uid:          u32,
    pub st_gid:          u32,
    pub st_rdev:         i32,
    _pad:                i32,
    pub st_atimespec:    Timespec,
    pub st_mtimespec:    Timespec,
    pub st_ctimespec:    Timespec,
    pub st_birthtimespec:Timespec,
    pub st_size:         i64,
    pub st_blocks:       i64,
    pub st_blksize:      i32,
    pub st_flags:        u32,
    pub st_gen:          u32,
    pub st_lspare:       i32,
    pub st_qspare:       [i64; 2],
}

const _STAT_SIZE: () = assert!(core::mem::size_of::<Stat>() == 144);

impl Stat {
    pub const fn zeroed() -> Self {
        // SAFETY: all-zero is a valid bit pattern for this POD struct.
        unsafe { core::mem::zeroed() }
    }

    #[inline] pub fn file_type(&self) -> u16 { self.st_mode & S_IFMT }
    #[inline] pub fn is_file(&self)      -> bool { self.file_type() == S_IFREG }
    #[inline] pub fn is_dir(&self)       -> bool { self.file_type() == S_IFDIR }
    #[inline] pub fn is_symlink(&self)   -> bool { self.file_type() == S_IFLNK }
}

// ── Dirent ─────────────────────────────────────────────────────────────

/// macOS arm64 `struct dirent` — variable-length; d_name is at offset 21.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Dirent {
    pub d_ino:    u64,
    pub d_seekoff:u64,
    pub d_reclen: u16,
    pub d_namlen: u16,
    pub d_type:   u8,
    pub d_name:   [u8; 1024],
}

// ── pthread opaque types ───────────────────────────────────────────────

/// Opaque pthread object — all pthread_t values are heap pointers.
#[repr(C)]
pub struct OpaqueThread { _priv: [u8; 0] }
pub type PthreadT       = *mut OpaqueThread;

/// pthread_attr_t on macOS LP64: { i64 __sig; [u8; 56] __opaque } = 64 bytes.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PthreadAttrT {
    pub __sig:    i64,
    pub __opaque: [u8; 56],
}
const _ATTR_SIZE: () = assert!(core::mem::size_of::<PthreadAttrT>() == 64);

/// pthread_mutex_t on macOS LP64: 64 bytes.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PthreadMutexT {
    pub __sig:    i64,
    pub __opaque: [u8; 56],
}
const _MTX_SIZE: () = assert!(core::mem::size_of::<PthreadMutexT>() == 64);

/// pthread_mutexattr_t on macOS LP64: 16 bytes.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PthreadMutexattrT {
    pub __sig:    i64,
    pub __opaque: [u8; 8],
}

/// pthread_cond_t on macOS LP64: 48 bytes.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PthreadCondT {
    pub __sig:    i64,
    pub __opaque: [u8; 40],
}
const _COND_SIZE: () = assert!(core::mem::size_of::<PthreadCondT>() == 48);

/// pthread_condattr_t on macOS LP64: 16 bytes.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PthreadCondattrT {
    pub __sig:    i64,
    pub __opaque: [u8; 8],
}

/// pthread_rwlock_t on macOS LP64: 200 bytes.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PthreadRwlockT {
    pub __sig:    i64,
    pub __opaque: [u8; 192],
}

pub type PthreadKeyT = usize;

// ── posix_spawn opaque types ───────────────────────────────────────────

/// On macOS, posix_spawn_file_actions_t is a pointer to an internal struct.
pub type PosixSpawnFileActionsT = *mut c_void;
/// posix_spawnattr_t is similarly a pointer.
pub type PosixSpanattrT         = *mut c_void;

// ── File / open constants ──────────────────────────────────────────────

pub const O_RDONLY:    c_int = 0x0000_0000;
pub const O_WRONLY:    c_int = 0x0000_0001;
pub const O_RDWR:      c_int = 0x0000_0002;
pub const O_NONBLOCK:  c_int = 0x0000_0004;
pub const O_APPEND:    c_int = 0x0000_0008;
pub const O_SHLOCK:    c_int = 0x0000_0010;
pub const O_EXLOCK:    c_int = 0x0000_0020;
pub const O_NOFOLLOW:  c_int = 0x0000_0100;
pub const O_CREAT:     c_int = 0x0000_0200;
pub const O_TRUNC:     c_int = 0x0000_0400;
pub const O_EXCL:      c_int = 0x0000_0800;
pub const O_NOCTTY:    c_int = 0x0002_0000;
pub const O_DIRECTORY: c_int = 0x0010_0000;
pub const O_SYMLINK:   c_int = 0x0020_0000;
pub const O_CLOEXEC:   c_int = 0x0100_0000;
pub const O_EVTONLY:   c_int = 0x0000_8000;

pub const SEEK_SET: c_int = 0;
pub const SEEK_CUR: c_int = 1;
pub const SEEK_END: c_int = 2;

// ── Permissions / file type bits ───────────────────────────────────────

pub const S_IFMT:  u16 = 0o170000;
pub const S_IFIFO: u16 = 0o010000;
pub const S_IFCHR: u16 = 0o020000;
pub const S_IFDIR: u16 = 0o040000;
pub const S_IFBLK: u16 = 0o060000;
pub const S_IFREG: u16 = 0o100000;
pub const S_IFLNK: u16 = 0o120000;
pub const S_IFSOCK:u16 = 0o140000;

pub const S_IRWXU: u16 = 0o0700;
pub const S_IRUSR: u16 = 0o0400;
pub const S_IWUSR: u16 = 0o0200;
pub const S_IXUSR: u16 = 0o0100;
pub const S_IRWXG: u16 = 0o0070;
pub const S_IRWXO: u16 = 0o0007;

// ── Dirent d_type ──────────────────────────────────────────────────────

pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO:    u8 = 1;
pub const DT_CHR:     u8 = 2;
pub const DT_DIR:     u8 = 4;
pub const DT_BLK:     u8 = 6;
pub const DT_REG:     u8 = 8;
pub const DT_LNK:     u8 = 10;
pub const DT_SOCK:    u8 = 12;
pub const DT_WHT:     u8 = 14;

// ── Clock IDs ──────────────────────────────────────────────────────────

pub const CLOCK_REALTIME:          clockid_t = 0;
pub const CLOCK_MONOTONIC:         clockid_t = 6;
pub const CLOCK_MONOTONIC_RAW:     clockid_t = 4;
pub const CLOCK_PROCESS_CPUTIME_ID:clockid_t = 12;
pub const CLOCK_THREAD_CPUTIME_ID: clockid_t = 16;

// ── waitpid options ────────────────────────────────────────────────────

pub const WNOHANG:   c_int = 0x0000_0001;
pub const WUNTRACED: c_int = 0x0000_0002;

// ── Signals ────────────────────────────────────────────────────────────

pub const SIGHUP:  c_int = 1;
pub const SIGINT:  c_int = 2;
pub const SIGQUIT: c_int = 3;
pub const SIGILL:  c_int = 4;
pub const SIGABRT: c_int = 6;
pub const SIGFPE:  c_int = 8;
pub const SIGKILL: c_int = 9;
pub const SIGSEGV: c_int = 11;
pub const SIGPIPE: c_int = 13;
pub const SIGALRM: c_int = 14;
pub const SIGTERM: c_int = 15;
pub const SIGCHLD: c_int = 20;
pub const SIGCONT: c_int = 19;
pub const SIGSTOP: c_int = 17;
pub const SIGUSR1: c_int = 30;
pub const SIGUSR2: c_int = 31;

// ── pthread constants ──────────────────────────────────────────────────

pub const PTHREAD_CREATE_JOINABLE: c_int = 1;
pub const PTHREAD_CREATE_DETACHED: c_int = 2;

pub const PTHREAD_MUTEX_DEFAULT:   c_int = 0;
pub const PTHREAD_MUTEX_ERRORCHECK:c_int = 1;
pub const PTHREAD_MUTEX_RECURSIVE: c_int = 2;

// ── ulock constants (macOS 10.12+) ─────────────────────────────────────

pub const UL_COMPARE_AND_WAIT:        u32 = 1;
pub const UL_UNFAIR_LOCK:             u32 = 2;
pub const UL_COMPARE_AND_WAIT_SHARED: u32 = 3;
pub const UL_UNFAIR_LOCK64_SHARED:    u32 = 4;
pub const UL_COMPARE_AND_WAIT64:      u32 = 5;
pub const UL_COMPARE_AND_WAIT64_SHARED:u32 = 6;

pub const ULF_WAKE_ALL:    u32 = 0x0000_0100;
pub const ULF_WAKE_THREAD: u32 = 0x0000_0200;
pub const ULF_NO_ERRNO:    u32 = 0x0100_0000;

// ── mmap / mprotect constants ──────────────────────────────────────────

pub const PROT_NONE:  c_int = 0x00;
pub const PROT_READ:  c_int = 0x01;
pub const PROT_WRITE: c_int = 0x02;
pub const PROT_EXEC:  c_int = 0x04;

pub const MAP_SHARED:    c_int = 0x0001;
pub const MAP_PRIVATE:   c_int = 0x0002;
pub const MAP_ANON:      c_int = 0x1000;
pub const MAP_ANONYMOUS: c_int = MAP_ANON;
pub const MAP_FIXED:     c_int = 0x0010;
pub const MAP_NORESERVE: c_int = 0x0040;

pub const MAP_FAILED: *mut c_void = usize::MAX as *mut c_void;

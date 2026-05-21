//! Safe filesystem wrappers.
//!
//! All functions wrap the raw FFI with errno → OsError conversion.
//! Paths are `&[u8]` (NUL-free byte slices); the wrappers append a NUL
//! terminator on the stack for the syscall and verify no embedded NULs.

use crate::ffi::fs as raw;
use crate::ffi::types::*;
use crate::error::{OsError, Result};

// ── RAII file descriptor ────────────────────────────────────────────────

/// An owned file descriptor that closes on drop.
pub struct File {
    fd: c_int,
}

impl File {
    /// Open an existing file for reading.
    #[inline]
    pub fn open_read(path: &[u8]) -> Result<Self> {
        open_fd(path, O_RDONLY | O_CLOEXEC, 0)
    }

    /// Open or create a file for writing, truncating it.
    #[inline]
    pub fn create(path: &[u8], mode: u16) -> Result<Self> {
        open_fd(path, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, mode)
    }

    /// Open or create a file for reading and writing.
    #[inline]
    pub fn open_rdwr(path: &[u8], flags: c_int, mode: u16) -> Result<Self> {
        open_fd(path, O_RDWR | flags | O_CLOEXEC, mode)
    }

    /// Wrap an already-open fd (takes ownership; must not be closed elsewhere).
    ///
    /// # Safety
    /// `fd` must be a valid open file descriptor.
    #[inline]
    pub unsafe fn from_raw_fd(fd: c_int) -> Self { Self { fd } }

    #[inline] pub fn fd(&self) -> c_int { self.fd }

    /// Read up to `buf.len()` bytes.  Returns the number of bytes read
    /// (0 = end of file).
    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let n = unsafe { raw::read(self.fd, buf.as_mut_ptr() as *mut c_void, buf.len()) };
        if n < 0 { Err(OsError::last()) } else { Ok(n as usize) }
    }

    /// Write all of `buf`, retrying on EINTR.
    pub fn write_all(&self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            let n = unsafe { raw::write(self.fd, buf.as_ptr() as *const c_void, buf.len()) };
            if n < 0 {
                let e = OsError::last();
                if e.is_interrupted() { continue; }
                return Err(e);
            }
            buf = &buf[n as usize..];
        }
        Ok(())
    }

    /// Write, returning the byte count written (may be less than buf).
    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        let n = unsafe { raw::write(self.fd, buf.as_ptr() as *const c_void, buf.len()) };
        if n < 0 { Err(OsError::last()) } else { Ok(n as usize) }
    }

    pub fn pread(&self, buf: &mut [u8], offset: i64) -> Result<usize> {
        let n = unsafe { raw::pread(self.fd, buf.as_mut_ptr() as *mut c_void, buf.len(), offset) };
        if n < 0 { Err(OsError::last()) } else { Ok(n as usize) }
    }

    pub fn pwrite(&self, buf: &[u8], offset: i64) -> Result<usize> {
        let n = unsafe { raw::pwrite(self.fd, buf.as_ptr() as *const c_void, buf.len(), offset) };
        if n < 0 { Err(OsError::last()) } else { Ok(n as usize) }
    }

    pub fn seek(&self, offset: i64, whence: c_int) -> Result<i64> {
        let pos = unsafe { raw::lseek(self.fd, offset, whence) };
        if pos < 0 { Err(OsError::last()) } else { Ok(pos) }
    }

    pub fn seek_set(&self, pos: u64) -> Result<u64> {
        self.seek(pos as i64, SEEK_SET).map(|p| p as u64)
    }

    pub fn seek_end(&self, offset: i64) -> Result<i64> {
        self.seek(offset, SEEK_END)
    }

    pub fn metadata(&self) -> Result<Stat> {
        let mut st = Stat::zeroed();
        let r = unsafe { raw::fstat(self.fd, &mut st) };
        if r < 0 { Err(OsError::last()) } else { Ok(st) }
    }

    pub fn sync(&self) -> Result<()> {
        let r = unsafe { raw::fsync(self.fd) };
        if r < 0 { Err(OsError::last()) } else { Ok(()) }
    }

    pub fn set_len(&self, size: u64) -> Result<()> {
        let r = unsafe { raw::ftruncate(self.fd, size as i64) };
        if r < 0 { Err(OsError::last()) } else { Ok(()) }
    }

    pub fn dup(&self) -> Result<Self> {
        let fd2 = unsafe { raw::dup(self.fd) };
        if fd2 < 0 { Err(OsError::last()) } else { Ok(unsafe { Self::from_raw_fd(fd2) }) }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        unsafe { raw::close(self.fd); }
    }
}

// ── Internal: open a fd from a byte-slice path ──────────────────────────

fn open_fd(path: &[u8], flags: c_int, mode: u16) -> Result<File> {
    let mut buf = [0u8; 4096];
    let p = nul_path(path, &mut buf)?;
    let fd = unsafe { raw::open(p, flags, mode as c_int) };
    if fd < 0 { Err(OsError::last()) } else { Ok(unsafe { File::from_raw_fd(fd) }) }
}

// ── Path operations ─────────────────────────────────────────────────────

/// Retrieve file metadata without following symlinks.
pub fn metadata(path: &[u8]) -> Result<Stat> {
    let mut buf = [0u8; 4096];
    let mut st = Stat::zeroed();
    let p = nul_path(path, &mut buf)?;
    let r = unsafe { raw::stat(p, &mut st) };
    if r < 0 { Err(OsError::last()) } else { Ok(st) }
}

/// Retrieve file metadata, following symlinks.
pub fn lstat(path: &[u8]) -> Result<Stat> {
    let mut buf = [0u8; 4096];
    let mut st = Stat::zeroed();
    let p = nul_path(path, &mut buf)?;
    let r = unsafe { raw::lstat(p, &mut st) };
    if r < 0 { Err(OsError::last()) } else { Ok(st) }
}

/// Create a directory.
pub fn mkdir(path: &[u8], mode: u16) -> Result<()> {
    let mut buf = [0u8; 4096];
    let p = nul_path(path, &mut buf)?;
    let r = unsafe { raw::mkdir(p, mode) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

/// Remove an empty directory.
pub fn rmdir(path: &[u8]) -> Result<()> {
    let mut buf = [0u8; 4096];
    let p = nul_path(path, &mut buf)?;
    let r = unsafe { raw::rmdir(p) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

/// Remove a file.
pub fn unlink(path: &[u8]) -> Result<()> {
    let mut buf = [0u8; 4096];
    let p = nul_path(path, &mut buf)?;
    let r = unsafe { raw::unlink(p) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

/// Rename (or move) a file or directory.
pub fn rename(from: &[u8], to: &[u8]) -> Result<()> {
    let mut fb = [0u8; 4096];
    let mut tb = [0u8; 4096];
    let fp = nul_path(from, &mut fb)?;
    let tp = nul_path(to, &mut tb)?;
    let r = unsafe { raw::rename(fp, tp) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

/// Create a hard link.
pub fn link(existing: &[u8], new: &[u8]) -> Result<()> {
    let mut eb = [0u8; 4096];
    let mut nb = [0u8; 4096];
    let ep = nul_path(existing, &mut eb)?;
    let np = nul_path(new, &mut nb)?;
    let r = unsafe { raw::link(ep, np) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

/// Create a symbolic link (`path1` → `path2`).
pub fn symlink(target: &[u8], link_path: &[u8]) -> Result<()> {
    let mut tb = [0u8; 4096];
    let mut lb = [0u8; 4096];
    let tp = nul_path(target, &mut tb)?;
    let lp = nul_path(link_path, &mut lb)?;
    let r = unsafe { raw::symlink(tp, lp) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

/// Read the target of a symbolic link.  Returns bytes written to `buf`.
pub fn readlink<'b>(path: &[u8], buf: &'b mut [u8]) -> Result<&'b [u8]> {
    let mut pb = [0u8; 4096];
    let p = nul_path(path, &mut pb)?;
    let n = unsafe { raw::readlink(p, buf.as_mut_ptr() as *mut c_char, buf.len()) };
    if n < 0 { Err(OsError::last()) } else { Ok(&buf[..n as usize]) }
}

/// Create an anonymous pipe.  Returns (read_fd, write_fd).
pub fn pipe() -> Result<(File, File)> {
    let mut fds = [0i32; 2];
    let r = unsafe { raw::pipe(&mut fds) };
    if r < 0 {
        Err(OsError::last())
    } else {
        Ok((unsafe { File::from_raw_fd(fds[0]) }, unsafe { File::from_raw_fd(fds[1]) }))
    }
}

/// Map a region of anonymous memory.
pub fn mmap_anon(len: usize, prot: c_int) -> Result<*mut u8> {
    let p = unsafe {
        raw::mmap(core::ptr::null_mut(), len, prot, MAP_PRIVATE | MAP_ANON, -1, 0)
    };
    if p == MAP_FAILED { Err(OsError::last()) } else { Ok(p as *mut u8) }
}

/// Unmap a previously mapped region.
///
/// # Safety
/// `addr` must be a pointer returned by `mmap`, and `len` must match.
pub unsafe fn munmap(addr: *mut u8, len: usize) -> Result<()> {
    let r = raw::munmap(addr as *mut c_void, len);
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

// ── Internal helper ─────────────────────────────────────────────────────

use crate::error::{EINVAL};

/// Copy `path` bytes into `buf` and append a NUL terminator.
/// Returns a `*const c_char` pointing into `buf`.
/// Fails with EINVAL if path contains an embedded NUL or is too long.
pub(crate) fn nul_path<'b>(path: &[u8], buf: &'b mut [u8]) -> Result<*const c_char> {
    if path.len() >= buf.len() {
        return Err(OsError(crate::error::ENAMETOOLONG));
    }
    if path.contains(&0u8) {
        return Err(OsError(EINVAL));
    }
    buf[..path.len()].copy_from_slice(path);
    buf[path.len()] = 0;
    Ok(buf.as_ptr() as *const c_char)
}

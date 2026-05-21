//! Safe directory-iterator wrapper.
//!
//! `Dir` is an RAII handle that closes the underlying DIR stream on drop.
//! `next()` yields `DirEntry` values, skipping the "." and ".." pseudo-entries.

use crate::ffi::fs as raw;
use crate::ffi::types::*;
use crate::error::{OsError, Result};
use crate::fs::nul_path;

/// An owned DIR stream.  Closed automatically on drop.
pub struct Dir(*mut c_void);

impl Dir {
    /// Open the directory at `path` (NUL-free byte slice).
    pub fn open(path: &[u8]) -> Result<Self> {
        let mut buf = [0u8; 4096];
        let p = nul_path(path, &mut buf)?;
        let d = unsafe { raw::opendir(p) };
        if d.is_null() { Err(OsError::last()) } else { Ok(Self(d)) }
    }

    /// Return the next entry, or `None` at end-of-directory.
    ///
    /// "." and ".." are skipped automatically.
    pub fn next(&mut self) -> Option<DirEntry> {
        loop {
            let ent = unsafe { raw::readdir(self.0) };
            if ent.is_null() { return None; }
            let e = unsafe { &*ent };
            let name_len = e.d_namlen as usize;
            let name_bytes = &e.d_name[..name_len];
            if name_bytes == b"." || name_bytes == b".." { continue; }

            let mut name = [0u8; 256];
            let copy_len = name_len.min(255);
            name[..copy_len].copy_from_slice(&e.d_name[..copy_len]);

            return Some(DirEntry {
                ino: e.d_ino,
                file_type: e.d_type,
                name,
                name_len: copy_len,
            });
        }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        unsafe { raw::closedir(self.0); }
    }
}

/// A single directory entry returned by `Dir::next()`.
pub struct DirEntry {
    pub ino:       u64,
    pub file_type: u8,
    name:          [u8; 256],
    name_len:      usize,
}

impl DirEntry {
    /// The entry name as a byte slice (not NUL-terminated).
    #[inline]
    pub fn name(&self) -> &[u8] { &self.name[..self.name_len] }
}

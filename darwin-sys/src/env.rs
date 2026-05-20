//! Safe environment and working-directory wrappers.

use crate::ffi::misc as raw;
use crate::ffi::types::*;
use crate::error::{OsError, Result};

// ── Environment variables ────────────────────────────────────────────────

/// Look up an environment variable by NUL-terminated name.
///
/// Returns a pointer to the NUL-terminated value string, or `None` if unset.
/// The pointer is valid until the next `setenv`/`unsetenv` call.
///
/// # Safety
/// The caller must not modify the returned bytes, and must not hold the
/// pointer across any call to `setenv`, `unsetenv`, or `putenv`.
pub unsafe fn getenv_raw(name: *const c_char) -> Option<*const c_char> {
    let p = raw::getenv(name);
    if p.is_null() { None } else { Some(p) }
}

/// Copy the value of an environment variable into `buf`.
/// Returns the bytes written (excluding NUL) on success, or `None` if unset.
pub fn getenv(name: &[u8], buf: &mut [u8]) -> Option<usize> {
    if name.contains(&0u8) { return None; }
    let mut nb = [0u8; 256];
    if name.len() >= nb.len() { return None; }
    nb[..name.len()].copy_from_slice(name);
    nb[name.len()] = 0;
    unsafe {
        let p = raw::getenv(nb.as_ptr() as *const c_char);
        if p.is_null() { return None; }
        let mut len = 0usize;
        while *p.add(len) != 0 { len += 1; }
        if len >= buf.len() { return None; }
        core::ptr::copy_nonoverlapping(p as *const u8, buf.as_mut_ptr(), len);
        buf[len] = 0;
        Some(len)
    }
}

/// Set an environment variable.  Overwrites if `overwrite` is true.
pub fn setenv(name: &[u8], value: &[u8], overwrite: bool) -> Result<()> {
    let mut nb = [0u8; 256];
    let mut vb = [0u8; 4096];
    if name.len() >= nb.len() || name.contains(&0u8) {
        return Err(OsError(crate::error::EINVAL));
    }
    if value.len() >= vb.len() || value.contains(&0u8) {
        return Err(OsError(crate::error::EINVAL));
    }
    nb[..name.len()].copy_from_slice(name);
    nb[name.len()] = 0;
    vb[..value.len()].copy_from_slice(value);
    vb[value.len()] = 0;
    let r = unsafe { raw::setenv(nb.as_ptr() as *const c_char, vb.as_ptr() as *const c_char, overwrite as c_int) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

/// Unset an environment variable.
pub fn unsetenv(name: &[u8]) -> Result<()> {
    let mut nb = [0u8; 256];
    if name.len() >= nb.len() || name.contains(&0u8) {
        return Err(OsError(crate::error::EINVAL));
    }
    nb[..name.len()].copy_from_slice(name);
    nb[name.len()] = 0;
    let r = unsafe { raw::unsetenv(nb.as_ptr() as *const c_char) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

/// Return a pointer to the process environment array (`environ`).
///
/// The array is NULL-terminated; each element is a `KEY=VALUE\0` string.
/// # Safety
/// Do not modify the pointed-to strings.  The pointer is invalidated by
/// `setenv`/`unsetenv`.
pub unsafe fn environ() -> *const *const c_char {
    raw::environ
}

// ── Working directory ────────────────────────────────────────────────────

/// Get the current working directory into `buf`.
/// Returns the bytes written (excluding NUL) on success.
pub fn getcwd(buf: &mut [u8]) -> Result<usize> {
    let p = unsafe { raw::getcwd(buf.as_mut_ptr() as *mut c_char, buf.len()) };
    if p.is_null() {
        return Err(OsError::last());
    }
    let mut len = 0usize;
    while len < buf.len() && buf[len] != 0 { len += 1; }
    Ok(len)
}

/// Change the current working directory.
pub fn chdir(path: &[u8]) -> Result<()> {
    let mut buf = [0u8; 4096];
    if path.len() >= buf.len() || path.contains(&0u8) {
        return Err(OsError(crate::error::EINVAL));
    }
    buf[..path.len()].copy_from_slice(path);
    buf[path.len()] = 0;
    let r = unsafe { raw::chdir(buf.as_ptr() as *const c_char) };
    if r < 0 { Err(OsError::last()) } else { Ok(()) }
}

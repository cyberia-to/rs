//! Cryptographic randomness via `getentropy`.

use crate::ffi::misc as raw;
use crate::ffi::types::*;
use crate::error::{OsError, Result};

/// Fill `buf` with cryptographically-secure random bytes.
///
/// `getentropy` limits requests to 256 bytes per call; this wrapper loops
/// to satisfy larger requests.
pub fn fill(buf: &mut [u8]) -> Result<()> {
    const MAX: usize = 256;
    let mut off = 0;
    while off < buf.len() {
        let chunk = (buf.len() - off).min(MAX);
        let r = unsafe { raw::getentropy(buf[off..].as_mut_ptr() as *mut c_void, chunk) };
        if r < 0 { return Err(OsError::last()); }
        off += chunk;
    }
    Ok(())
}

/// Return a random `u64`.
pub fn random_u64() -> Result<u64> {
    let mut buf = [0u8; 8];
    fill(&mut buf)?;
    Ok(u64::from_ne_bytes(buf))
}

/// Return a random `u32`.
pub fn random_u32() -> Result<u32> {
    let mut buf = [0u8; 4];
    fill(&mut buf)?;
    Ok(u32::from_ne_bytes(buf))
}

//! Fixed-capacity UTF-8 string with inline storage.

use super::BoundedVec;

/// Fixed-capacity UTF-8 string stored inline.
///
/// Backed by `BoundedVec<u8, N>` with a UTF-8 validity invariant.
/// `N` is the capacity in bytes.
pub struct ArrayString<const N: usize> {
    bytes: BoundedVec<u8, N>,
}

impl<const N: usize> ArrayString<N> {
    /// Create an empty `ArrayString`.
    pub const fn new() -> Self {
        Self { bytes: BoundedVec::new() }
    }

    /// Returns the length in bytes.
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns `true` if empty.
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Returns the byte capacity.
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Returns the string as a `&str`.
    pub fn as_str(&self) -> &str {
        // SAFETY: We maintain the invariant that bytes contains valid UTF-8.
        unsafe { core::str::from_utf8_unchecked(self.bytes.as_slice()) }
    }

    /// Try to create from a string slice. Returns `None` if it exceeds capacity.
    pub fn try_from_str(s: &str) -> Option<Self> {
        if s.len() > N {
            return None;
        }
        let mut result = Self::new();
        for &b in s.as_bytes() {
            let _ = result.bytes.try_push(b);
        }
        Some(result)
    }

    /// Try to append a string slice. Returns `Err` if it would exceed capacity.
    #[allow(clippy::result_unit_err)]
    pub fn try_push_str(&mut self, s: &str) -> Result<(), ()> {
        if self.bytes.len() + s.len() > N {
            return Err(());
        }
        for &b in s.as_bytes() {
            let _ = self.bytes.try_push(b);
        }
        Ok(())
    }

    /// Try to append a single character. Returns `Err` if it would exceed capacity.
    #[allow(clippy::result_unit_err)]
    pub fn try_push_char(&mut self, c: char) -> Result<(), ()> {
        let mut buf = [0u8; 4];
        let encoded = c.encode_utf8(&mut buf);
        self.try_push_str(encoded)
    }

    /// Clear the string.
    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    /// Returns the underlying bytes as a slice.
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }
}

impl<const N: usize> Clone for ArrayString<N> {
    fn clone(&self) -> Self {
        Self { bytes: self.bytes.clone() }
    }
}

impl<const N: usize> PartialEq for ArrayString<N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl<const N: usize> Eq for ArrayString<N> {}

impl<const N: usize> PartialOrd for ArrayString<N> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<const N: usize> Ord for ArrayString<N> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl<const N: usize> core::fmt::Debug for ArrayString<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl<const N: usize> core::fmt::Display for ArrayString<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<const N: usize> Default for ArrayString<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> core::hash::Hash for ArrayString<N> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl<const N: usize> crate::StepReset for ArrayString<N> {
    fn reset(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let s: ArrayString<64> = ArrayString::new();
        assert!(s.is_empty());
        assert_eq!(s.as_str(), "");
    }

    #[test]
    fn try_from_str() {
        let s = ArrayString::<64>::try_from_str("hello").unwrap();
        assert_eq!(s.as_str(), "hello");
        assert_eq!(s.len(), 5);
    }

    #[test]
    fn try_from_str_too_long() {
        assert!(ArrayString::<4>::try_from_str("hello").is_none());
    }

    #[test]
    fn push_str() {
        let mut s = ArrayString::<64>::new();
        s.try_push_str("hello").unwrap();
        s.try_push_str(" world").unwrap();
        assert_eq!(s.as_str(), "hello world");
    }

    #[test]
    fn push_str_overflow() {
        let mut s = ArrayString::<5>::new();
        s.try_push_str("hello").unwrap();
        assert!(s.try_push_str("!").is_err());
    }

    #[test]
    fn push_char() {
        let mut s = ArrayString::<16>::new();
        s.try_push_char('h').unwrap();
        s.try_push_char('i').unwrap();
        assert_eq!(s.as_str(), "hi");
    }

    #[test]
    fn unicode() {
        let s = ArrayString::<16>::try_from_str("\u{1F600}").unwrap();
        assert_eq!(s.len(), 4);
        assert_eq!(s.as_str(), "\u{1F600}");
    }

    #[test]
    fn display() {
        let s = ArrayString::<64>::try_from_str("test").unwrap();
        let formatted = alloc::format!("{}", s);
        assert_eq!(formatted, "test");
    }

    #[test]
    fn ordering() {
        let a = ArrayString::<8>::try_from_str("abc").unwrap();
        let b = ArrayString::<8>::try_from_str("abd").unwrap();
        assert!(a < b);
    }

    #[test]
    fn clear() {
        let mut s = ArrayString::<64>::try_from_str("hello").unwrap();
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn step_reset() {
        use crate::StepReset;
        let mut s = ArrayString::<64>::try_from_str("hello").unwrap();
        s.reset();
        assert!(s.is_empty());
    }
}

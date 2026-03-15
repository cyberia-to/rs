//! Fixed-capacity vector with inline `MaybeUninit` storage.

use core::mem::MaybeUninit;
use crate::core_types::BufMut;

/// Fixed-capacity vector stored inline (no heap allocation).
///
/// Uses `[MaybeUninit<T>; N]` for storage. Elements are initialized
/// up to `len`, and uninitialized beyond. `try_push` returns `Err`
/// when full.
pub struct BoundedVec<T, const N: usize> {
    data: [MaybeUninit<T>; N],
    len: usize,
}

impl<T, const N: usize> BoundedVec<T, N> {
    /// Create an empty `BoundedVec`.
    pub const fn new() -> Self {
        Self {
            // SAFETY: An array of MaybeUninit does not require initialization.
            data: unsafe { MaybeUninit::<[MaybeUninit<T>; N]>::uninit().assume_init() },
            len: 0,
        }
    }

    /// Returns the number of elements.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if empty.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the compile-time capacity.
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Returns `true` if the vec is at capacity.
    pub const fn is_full(&self) -> bool {
        self.len == N
    }

    /// Try to append an element. Returns `Err(value)` if full.
    pub fn try_push(&mut self, value: T) -> Result<(), T> {
        if self.len >= N {
            return Err(value);
        }
        self.data[self.len] = MaybeUninit::new(value);
        self.len += 1;
        Ok(())
    }

    /// Remove and return the last element, or `None` if empty.
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        // SAFETY: Element at self.len was initialized.
        Some(unsafe { self.data[self.len].assume_init_read() })
    }

    /// Try to insert at `index`, shifting elements right.
    /// Returns `Err(value)` if full or index out of bounds.
    pub fn try_insert(&mut self, index: usize, value: T) -> Result<(), T> {
        if index > self.len || self.len >= N {
            return Err(value);
        }
        if index < self.len {
            // SAFETY: We have verified bounds. Shift elements right by one.
            unsafe {
                let ptr = self.data.as_mut_ptr().add(index);
                core::ptr::copy(ptr, ptr.add(1), self.len - index);
            }
        }
        self.data[index] = MaybeUninit::new(value);
        self.len += 1;
        Ok(())
    }

    /// Remove the element at `index`, shifting elements left.
    /// Returns `None` if index is out of bounds.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.len {
            return None;
        }
        // SAFETY: Element at index is initialized.
        let value = unsafe { self.data[index].assume_init_read() };
        self.len -= 1;
        if index < self.len {
            // SAFETY: Shift remaining elements left.
            unsafe {
                let ptr = self.data.as_mut_ptr().add(index);
                core::ptr::copy(ptr.add(1), ptr, self.len - index);
            }
        }
        Some(value)
    }

    /// Get a reference to the element at `index`.
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }
        // SAFETY: Elements below len are initialized.
        Some(unsafe { self.data[index].assume_init_ref() })
    }

    /// Get a mutable reference to the element at `index`.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.len {
            return None;
        }
        // SAFETY: Elements below len are initialized.
        Some(unsafe { self.data[index].assume_init_mut() })
    }

    /// Returns a slice of the initialized elements.
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: Elements [0..len) are initialized and contiguous.
        unsafe { core::slice::from_raw_parts(self.data.as_ptr() as *const T, self.len) }
    }

    /// Returns a mutable slice of the initialized elements.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: Elements [0..len) are initialized and contiguous.
        unsafe { core::slice::from_raw_parts_mut(self.data.as_mut_ptr() as *mut T, self.len) }
    }

    /// Iterate over references.
    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.as_slice().iter()
    }

    /// Iterate over mutable references.
    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, T> {
        self.as_mut_slice().iter_mut()
    }

    /// Remove all elements, dropping each one.
    pub fn clear(&mut self) {
        while self.pop().is_some() {}
    }

    /// Truncate to `new_len`, dropping excess elements.
    pub fn truncate(&mut self, new_len: usize) {
        while self.len > new_len {
            self.pop();
        }
    }

    /// Returns the last element reference, if any.
    pub fn last(&self) -> Option<&T> {
        if self.len == 0 {
            None
        } else {
            self.get(self.len - 1)
        }
    }
}

impl<T: Clone, const N: usize> Clone for BoundedVec<T, N> {
    fn clone(&self) -> Self {
        let mut new = Self::new();
        for item in self.iter() {
            let _ = new.try_push(item.clone());
        }
        new
    }
}

impl<T: PartialEq, const N: usize> PartialEq for BoundedVec<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Eq, const N: usize> Eq for BoundedVec<T, N> {}

impl<T: core::fmt::Debug, const N: usize> core::fmt::Debug for BoundedVec<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T, const N: usize> Drop for BoundedVec<T, N> {
    fn drop(&mut self) {
        self.clear();
    }
}

impl<const N: usize> BufMut for BoundedVec<u8, N> {
    fn put_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            let _ = self.try_push(b);
        }
    }
}

impl<T, const N: usize> Default for BoundedVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> crate::StepReset for BoundedVec<T, N> {
    fn reset(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let v: BoundedVec<u32, 8> = BoundedVec::new();
        assert!(v.is_empty());
        assert_eq!(v.len(), 0);
        assert_eq!(v.capacity(), 8);
    }

    #[test]
    fn push_and_pop() {
        let mut v: BoundedVec<u32, 4> = BoundedVec::new();
        assert!(v.try_push(1).is_ok());
        assert!(v.try_push(2).is_ok());
        assert!(v.try_push(3).is_ok());
        assert!(v.try_push(4).is_ok());
        assert_eq!(v.try_push(5), Err(5));
        assert!(v.is_full());
        assert_eq!(v.pop(), Some(4));
        assert_eq!(v.pop(), Some(3));
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn insert_and_remove() {
        let mut v: BoundedVec<u32, 8> = BoundedVec::new();
        v.try_push(1).unwrap();
        v.try_push(3).unwrap();
        v.try_insert(1, 2).unwrap();
        assert_eq!(v.as_slice(), &[1, 2, 3]);
        assert_eq!(v.remove(1), Some(2));
        assert_eq!(v.as_slice(), &[1, 3]);
    }

    #[test]
    fn get() {
        let mut v: BoundedVec<u32, 4> = BoundedVec::new();
        v.try_push(10).unwrap();
        v.try_push(20).unwrap();
        assert_eq!(v.get(0), Some(&10));
        assert_eq!(v.get(1), Some(&20));
        assert_eq!(v.get(2), None);
    }

    #[test]
    fn iter() {
        let mut v: BoundedVec<u32, 4> = BoundedVec::new();
        v.try_push(1).unwrap();
        v.try_push(2).unwrap();
        let sum: u32 = v.iter().sum();
        assert_eq!(sum, 3);
    }

    #[test]
    fn clear() {
        let mut v: BoundedVec<u32, 4> = BoundedVec::new();
        v.try_push(1).unwrap();
        v.try_push(2).unwrap();
        v.clear();
        assert!(v.is_empty());
    }

    #[test]
    fn clone() {
        let mut v: BoundedVec<u32, 4> = BoundedVec::new();
        v.try_push(1).unwrap();
        v.try_push(2).unwrap();
        let v2 = v.clone();
        assert_eq!(v, v2);
    }

    #[test]
    fn truncate() {
        let mut v: BoundedVec<u32, 8> = BoundedVec::new();
        for i in 0..6 {
            v.try_push(i).unwrap();
        }
        v.truncate(3);
        assert_eq!(v.len(), 3);
        assert_eq!(v.as_slice(), &[0, 1, 2]);
    }

    #[test]
    fn buf_mut() {
        let mut v: BoundedVec<u8, 8> = BoundedVec::new();
        v.put_bytes(&[1, 2, 3]);
        v.put_bytes(&[4, 5]);
        assert_eq!(v.as_slice(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn buf_mut_overflow() {
        let mut v: BoundedVec<u8, 4> = BoundedVec::new();
        v.put_bytes(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(v.len(), 4);
        assert_eq!(v.as_slice(), &[1, 2, 3, 4]);
    }

    #[test]
    fn step_reset() {
        use crate::StepReset;
        let mut v: BoundedVec<u32, 4> = BoundedVec::new();
        v.try_push(1).unwrap();
        v.reset();
        assert!(v.is_empty());
    }
}

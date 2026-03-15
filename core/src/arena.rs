//! Fixed-capacity arena allocator with interior mutability.
//!
//! `Arena<T, N>` stores up to `N` values of type `T` in inline storage.
//! Allocation takes `&self` (shared reference) and returns `&mut T` using
//! interior mutability (`UnsafeCell` + atomic counter).
//!
//! All allocated values are dropped when the arena is dropped.
//! Individual deallocation is intentionally unsupported.

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Fixed-capacity arena allocator.
///
/// Provides bump allocation with O(1) `alloc` and bulk deallocation on drop.
/// The arena uses `UnsafeCell` for interior mutability, allowing allocation
/// through shared references.
///
/// # Thread safety
///
/// `Arena` is `Sync` when `T: Send`. The atomic counter ensures concurrent
/// allocations do not race, and each allocation returns a unique `&mut T`.
pub struct Arena<T, const N: usize> {
    storage: UnsafeCell<[MaybeUninit<T>; N]>,
    count: AtomicUsize,
}

// SAFETY: Arena hands out unique &mut T references from non-overlapping slots.
// The atomic counter prevents double-allocation of the same slot.
// T: Send is required because values may be sent to other threads via &mut T.
unsafe impl<T: Send, const N: usize> Sync for Arena<T, N> {}

// SAFETY: The arena itself can be sent between threads if T can be sent.
unsafe impl<T: Send, const N: usize> Send for Arena<T, N> {}

impl<T, const N: usize> Arena<T, N> {
    /// Create a new empty arena with capacity `N`.
    pub const fn new() -> Self {
        Self {
            // SAFETY: Array of MaybeUninit does not require initialization.
            storage: UnsafeCell::new(unsafe {
                MaybeUninit::<[MaybeUninit<T>; N]>::uninit().assume_init()
            }),
            count: AtomicUsize::new(0),
        }
    }

    /// Returns the number of allocated slots.
    pub fn count(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    /// Returns the compile-time capacity.
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Returns `true` if the arena has no more free slots.
    pub fn is_full(&self) -> bool {
        self.count() >= N
    }

    /// Returns `true` if no slots are allocated.
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// Allocate a value in the arena, returning a mutable reference.
    ///
    /// Returns `None` if the arena is full.
    ///
    /// # Safety rationale
    ///
    /// The atomic `fetch_add` ensures each caller gets a unique slot index.
    /// The returned `&mut T` is valid for the arena's lifetime and does not
    /// alias any other reference.
    #[allow(clippy::mut_from_ref)]
    pub fn alloc(&self, value: T) -> Option<&mut T> {
        let idx = self.count.fetch_add(1, Ordering::AcqRel);
        if idx >= N {
            // Roll back — arena was full.
            self.count.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        // SAFETY:
        // - idx is unique per caller (atomic fetch_add)
        // - idx < N (checked above)
        // - No other reference to this slot exists
        // - The UnsafeCell allows mutation through &self
        unsafe {
            let storage = &mut *self.storage.get();
            storage[idx] = MaybeUninit::new(value);
            Some(storage[idx].assume_init_mut())
        }
    }

    /// Iterate over all allocated values.
    ///
    /// # Safety
    ///
    /// This requires `&self` but accesses initialized slots.
    /// Safe only when no `&mut T` references from `alloc` are live.
    /// In practice, the caller must ensure exclusive access or accept
    /// that concurrent mutations may be visible.
    pub fn iter(&self) -> ArenaIter<'_, T, N> {
        ArenaIter {
            arena: self,
            index: 0,
            len: self.count(),
        }
    }

    /// Clear all allocated values, dropping each one.
    ///
    /// # Safety
    ///
    /// The caller must ensure no `&mut T` references from `alloc` are live.
    /// This method resets the count and drops all values.
    ///
    /// After calling `clear`, the arena can be reused.
    pub fn clear(&self) {
        let current = self.count.swap(0, Ordering::AcqRel);
        // SAFETY: We have exclusive logical access (swap set count to 0).
        // All slots [0..current) are initialized and must be dropped.
        unsafe {
            let storage = &mut *self.storage.get();
            for slot in storage.iter_mut().take(current) {
                slot.assume_init_drop();
            }
        }
    }
}

impl<T, const N: usize> Drop for Arena<T, N> {
    fn drop(&mut self) {
        let current = *self.count.get_mut();
        // SAFETY: In drop, we have exclusive access. All slots [0..current)
        // are initialized.
        let storage = self.storage.get_mut();
        for slot in storage.iter_mut().take(current) {
            unsafe {
                slot.assume_init_drop();
            }
        }
    }
}

impl<T, const N: usize> Default for Arena<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: core::fmt::Debug, const N: usize> core::fmt::Debug for Arena<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Arena")
            .field("count", &self.count())
            .field("capacity", &N)
            .finish()
    }
}

/// Iterator over allocated arena values.
pub struct ArenaIter<'a, T, const N: usize> {
    arena: &'a Arena<T, N>,
    index: usize,
    len: usize,
}

impl<'a, T, const N: usize> Iterator for ArenaIter<'a, T, N> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.len {
            return None;
        }
        // SAFETY: Slots [0..len) are initialized. Index is within bounds.
        let val = unsafe {
            let storage = &*self.arena.storage.get();
            storage[self.index].assume_init_ref()
        };
        self.index += 1;
        Some(val)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len - self.index;
        (remaining, Some(remaining))
    }
}

impl<'a, T, const N: usize> ExactSizeIterator for ArenaIter<'a, T, N> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_new_is_empty() {
        let arena: Arena<u32, 8> = Arena::new();
        assert!(arena.is_empty());
        assert_eq!(arena.count(), 0);
        assert_eq!(arena.capacity(), 8);
    }

    #[test]
    fn arena_alloc_basic() {
        let arena: Arena<u32, 4> = Arena::new();
        let a = arena.alloc(10).unwrap();
        assert_eq!(*a, 10);
        let b = arena.alloc(20).unwrap();
        assert_eq!(*b, 20);
        assert_eq!(arena.count(), 2);
    }

    #[test]
    fn arena_alloc_returns_mut() {
        let arena: Arena<u32, 4> = Arena::new();
        let a = arena.alloc(10).unwrap();
        *a = 42;
        assert_eq!(*a, 42);
    }

    #[test]
    fn arena_full() {
        let arena: Arena<u32, 2> = Arena::new();
        arena.alloc(1).unwrap();
        arena.alloc(2).unwrap();
        assert!(arena.is_full());
        assert!(arena.alloc(3).is_none());
        assert_eq!(arena.count(), 2); // Count should not have increased
    }

    #[test]
    fn arena_iter() {
        let arena: Arena<u32, 8> = Arena::new();
        arena.alloc(10).unwrap();
        arena.alloc(20).unwrap();
        arena.alloc(30).unwrap();

        let values: alloc::vec::Vec<&u32> = arena.iter().collect();
        assert_eq!(values, alloc::vec![&10, &20, &30]);
    }

    #[test]
    fn arena_iter_exact_size() {
        let arena: Arena<u32, 8> = Arena::new();
        arena.alloc(1).unwrap();
        arena.alloc(2).unwrap();
        assert_eq!(arena.iter().len(), 2);
    }

    #[test]
    fn arena_clear() {
        let arena: Arena<u32, 8> = Arena::new();
        arena.alloc(1).unwrap();
        arena.alloc(2).unwrap();
        arena.clear();
        assert!(arena.is_empty());
        assert_eq!(arena.count(), 0);
        // Can allocate again after clear
        arena.alloc(3).unwrap();
        assert_eq!(arena.count(), 1);
    }

    #[test]
    fn arena_drop_runs() {
        use core::sync::atomic::{AtomicU32, Ordering};
        static DROP_COUNT: AtomicU32 = AtomicU32::new(0);

        struct Droppable;
        impl Drop for Droppable {
            fn drop(&mut self) {
                DROP_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }

        DROP_COUNT.store(0, Ordering::Relaxed);
        {
            let arena: Arena<Droppable, 8> = Arena::new();
            arena.alloc(Droppable).unwrap();
            arena.alloc(Droppable).unwrap();
            arena.alloc(Droppable).unwrap();
        }
        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn arena_zero_capacity() {
        let arena: Arena<u32, 0> = Arena::new();
        assert!(arena.is_full());
        assert!(arena.alloc(1).is_none());
    }

    #[test]
    fn arena_debug() {
        let arena: Arena<u32, 8> = Arena::new();
        arena.alloc(1).unwrap();
        let s = alloc::format!("{:?}", arena);
        assert!(s.contains("Arena"));
        assert!(s.contains("count: 1"));
    }
}

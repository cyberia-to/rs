//! Fixed-capacity sorted map backed by `BoundedVec`.

use super::BoundedVec;

/// Fixed-capacity sorted map backed by a `BoundedVec<(K, V), N>`.
///
/// Keys are maintained in sorted order. Lookups use binary search.
/// Insertion is O(N) due to shifting; lookups are O(log N).
pub struct BoundedMap<K, V, const N: usize> {
    entries: BoundedVec<(K, V), N>,
}

impl<K: Ord, V, const N: usize> BoundedMap<K, V, N> {
    /// Create an empty map.
    pub const fn new() -> Self {
        Self {
            entries: BoundedVec::new(),
        }
    }

    /// Returns the number of entries.
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if empty.
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the compile-time capacity.
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Returns `true` if the map is at capacity.
    pub const fn is_full(&self) -> bool {
        self.entries.is_full()
    }

    /// Find the index where `key` is or should be inserted.
    fn search_index(&self, key: &K) -> Result<usize, usize> {
        self.entries.as_slice().binary_search_by(|(k, _)| k.cmp(key))
    }

    /// Try to insert a key-value pair. If the key exists, the value is updated.
    /// Returns `Err((key, value))` if the map is full and the key is new.
    pub fn try_insert(&mut self, key: K, value: V) -> Result<Option<V>, (K, V)> {
        match self.search_index(&key) {
            Ok(idx) => {
                // Key exists — replace value.
                // SAFETY: idx is within bounds per binary_search result.
                let entry = unsafe { self.entries.get_mut(idx).unwrap_unchecked() };
                let old = core::mem::replace(&mut entry.1, value);
                Ok(Some(old))
            }
            Err(idx) => {
                match self.entries.try_insert(idx, (key, value)) {
                    Ok(()) => Ok(None),
                    Err((k, v)) => Err((k, v)),
                }
            }
        }
    }

    /// Get a reference to the value for `key`.
    pub fn get(&self, key: &K) -> Option<&V> {
        match self.search_index(key) {
            Ok(idx) => self.entries.get(idx).map(|(_, v)| v),
            Err(_) => None,
        }
    }

    /// Get a mutable reference to the value for `key`.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        match self.search_index(key) {
            Ok(idx) => self.entries.get_mut(idx).map(|(_, v)| v),
            Err(_) => None,
        }
    }

    /// Returns `true` if the key is present.
    pub fn contains_key(&self, key: &K) -> bool {
        self.search_index(key).is_ok()
    }

    /// Remove a key-value pair. Returns the value if the key was present.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        match self.search_index(key) {
            Ok(idx) => self.entries.remove(idx).map(|(_, v)| v),
            Err(_) => None,
        }
    }

    /// Iterate over `(key, value)` pairs in sorted order.
    pub fn iter(&self) -> core::slice::Iter<'_, (K, V)> {
        self.entries.iter()
    }

    /// Iterate over mutable `(key, value)` pairs in sorted order.
    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, (K, V)> {
        self.entries.iter_mut()
    }

    /// Remove all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get the entries as a slice of `(K, V)` pairs.
    pub fn as_slice(&self) -> &[(K, V)] {
        self.entries.as_slice()
    }
}

impl<K: Ord + Clone, V: Clone, const N: usize> Clone for BoundedMap<K, V, N> {
    fn clone(&self) -> Self {
        Self { entries: self.entries.clone() }
    }
}

impl<K: Ord + PartialEq, V: PartialEq, const N: usize> PartialEq for BoundedMap<K, V, N> {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl<K: Ord + Eq, V: Eq, const N: usize> Eq for BoundedMap<K, V, N> {}

impl<K: Ord + core::fmt::Debug, V: core::fmt::Debug, const N: usize> core::fmt::Debug
    for BoundedMap<K, V, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_map()
            .entries(self.iter().map(|(k, v)| (k, v)))
            .finish()
    }
}

impl<K: Ord, V, const N: usize> Default for BoundedMap<K, V, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord, V, const N: usize> crate::StepReset for BoundedMap<K, V, N> {
    fn reset(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let m: BoundedMap<u32, u32, 8> = BoundedMap::new();
        assert!(m.is_empty());
    }

    #[test]
    fn insert_and_get() {
        let mut m: BoundedMap<u32, &str, 8> = BoundedMap::new();
        assert_eq!(m.try_insert(3, "three").unwrap(), None);
        assert_eq!(m.try_insert(1, "one").unwrap(), None);
        assert_eq!(m.try_insert(2, "two").unwrap(), None);
        assert_eq!(m.get(&1), Some(&"one"));
        assert_eq!(m.get(&2), Some(&"two"));
        assert_eq!(m.get(&3), Some(&"three"));
        assert_eq!(m.get(&4), None);
    }

    #[test]
    fn insert_replaces() {
        let mut m: BoundedMap<u32, &str, 8> = BoundedMap::new();
        m.try_insert(1, "one").unwrap();
        let old = m.try_insert(1, "uno").unwrap();
        assert_eq!(old, Some("one"));
        assert_eq!(m.get(&1), Some(&"uno"));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn remove() {
        let mut m: BoundedMap<u32, &str, 8> = BoundedMap::new();
        m.try_insert(1, "one").unwrap();
        m.try_insert(2, "two").unwrap();
        assert_eq!(m.remove(&1), Some("one"));
        assert_eq!(m.get(&1), None);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn contains_key() {
        let mut m: BoundedMap<u32, u32, 8> = BoundedMap::new();
        m.try_insert(42, 100).unwrap();
        assert!(m.contains_key(&42));
        assert!(!m.contains_key(&43));
    }

    #[test]
    fn full() {
        let mut m: BoundedMap<u32, u32, 2> = BoundedMap::new();
        m.try_insert(1, 10).unwrap();
        m.try_insert(2, 20).unwrap();
        assert!(m.try_insert(3, 30).is_err());
        assert!(m.try_insert(1, 11).is_ok());
    }

    #[test]
    fn sorted_order() {
        let mut m: BoundedMap<u32, u32, 8> = BoundedMap::new();
        m.try_insert(5, 50).unwrap();
        m.try_insert(1, 10).unwrap();
        m.try_insert(3, 30).unwrap();
        let keys: alloc::vec::Vec<u32> = m.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, alloc::vec![1, 3, 5]);
    }

    #[test]
    fn clear() {
        let mut m: BoundedMap<u32, u32, 8> = BoundedMap::new();
        m.try_insert(1, 10).unwrap();
        m.clear();
        assert!(m.is_empty());
    }

    #[test]
    fn step_reset() {
        use crate::StepReset;
        let mut m: BoundedMap<u32, u32, 4> = BoundedMap::new();
        m.try_insert(1, 10).unwrap();
        m.reset();
        assert!(m.is_empty());
    }
}

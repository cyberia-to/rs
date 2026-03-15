//! Bounded MPMC (multi-producer, multi-consumer) channel.
//!
//! Lock-free ring buffer with atomic head/tail pointers.
//! Capacity is a const generic, consistent with the compile-time
//! bounds philosophy.
//!
//! ```ignore
//! let (tx, rx) = bounded_channel::<Transaction, 1000>();
//! tx.try_send(msg).ok();
//! let msg = rx.try_recv();
//! ```

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Error returned when a channel is full.
#[derive(Debug, PartialEq, Eq)]
pub struct Full<T>(
    /// The value that could not be sent.
    pub T,
);

impl<T> Full<T> {
    /// Unwrap the unsent value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

/// Internal shared state for the channel.
///
/// Uses a ring buffer with power-of-two sizing for efficient modular indexing.
/// The actual usable capacity is `N` slots; the buffer is `N` entries.
///
/// Head and tail are monotonically increasing indices. Wrapping is handled
/// by masking or modular arithmetic on access.
struct ChannelInner<T, const N: usize> {
    buffer: [UnsafeCell<MaybeUninit<T>>; N],
    head: AtomicUsize,   // next slot to write
    tail: AtomicUsize,   // next slot to read
    _sender_count: AtomicUsize,
    _receiver_count: AtomicUsize,
}

// SAFETY: The ring buffer slots are accessed at distinct indices by
// producers (head) and consumers (tail). The atomic operations ensure
// that no two threads access the same slot simultaneously in a conflicting way.
unsafe impl<T: Send, const N: usize> Sync for ChannelInner<T, N> {}
unsafe impl<T: Send, const N: usize> Send for ChannelInner<T, N> {}

#[allow(dead_code)]
impl<T, const N: usize> ChannelInner<T, N> {
    const fn new() -> Self {
        // SAFETY: Array of UnsafeCell<MaybeUninit<T>> does not require
        // initialization — MaybeUninit is explicitly uninitialized.
        Self {
            buffer: unsafe {
                MaybeUninit::<[UnsafeCell<MaybeUninit<T>>; N]>::uninit().assume_init()
            },
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            _sender_count: AtomicUsize::new(1),
            _receiver_count: AtomicUsize::new(1),
        }
    }

    fn try_send(&self, value: T) -> Result<(), Full<T>> {
        if N == 0 {
            return Err(Full(value));
        }

        loop {
            let head = self.head.load(Ordering::Acquire);
            let tail = self.tail.load(Ordering::Acquire);

            // Channel is full when head - tail == N
            if head.wrapping_sub(tail) >= N {
                return Err(Full(value));
            }

            // Try to claim the head slot
            if self
                .head
                .compare_exchange_weak(head, head.wrapping_add(1), Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let idx = head % N;
                // SAFETY: We have exclusive access to this slot via the CAS.
                unsafe {
                    (*self.buffer[idx].get()) = MaybeUninit::new(value);
                }
                return Ok(());
            }
            // CAS failed — another producer won. Retry.
            core::hint::spin_loop();
        }
    }

    fn try_recv(&self) -> Option<T> {
        if N == 0 {
            return None;
        }

        loop {
            let tail = self.tail.load(Ordering::Acquire);
            let head = self.head.load(Ordering::Acquire);

            // Channel is empty when tail == head
            if tail == head {
                return None;
            }

            // Try to claim the tail slot
            if self
                .tail
                .compare_exchange_weak(tail, tail.wrapping_add(1), Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let idx = tail % N;
                // SAFETY: This slot was written by a producer and is now
                // exclusively owned by this consumer via the CAS.
                let value = unsafe { (*self.buffer[idx].get()).assume_init_read() };
                return Some(value);
            }
            // CAS failed — another consumer won. Retry.
            core::hint::spin_loop();
        }
    }

    fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        head.wrapping_sub(tail)
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn is_full(&self) -> bool {
        self.len() >= N
    }
}

impl<T, const N: usize> Drop for ChannelInner<T, N> {
    fn drop(&mut self) {
        // Drop any remaining items in the buffer.
        let tail = *self.tail.get_mut();
        let head = *self.head.get_mut();
        let mut i = tail;
        while i != head {
            let idx = i % N;
            // SAFETY: Slots between tail and head are initialized.
            unsafe {
                self.buffer[idx].get_mut().assume_init_drop();
            }
            i = i.wrapping_add(1);
        }
    }
}

// =============================================================================
// Shared ownership via manual reference counting
// =============================================================================

// We use a raw pointer to a heap-allocated ChannelInner for shared ownership.
// In no_std without alloc, we provide a stack-based alternative.

/// Wrapper around the shared channel state.
///
/// In no_std + no alloc mode, the channel must be created with a static
/// or caller-managed backing store. For simplicity and correctness, we
/// always use the inner struct directly with reference counting.

#[cfg(any(feature = "std", test))]
mod shared {
    use super::*;
    use alloc::sync::Arc;

    /// Sending half of a bounded channel.
    pub struct Sender<T, const N: usize> {
        inner: Arc<ChannelInner<T, N>>,
    }

    /// Receiving half of a bounded channel.
    pub struct Receiver<T, const N: usize> {
        inner: Arc<ChannelInner<T, N>>,
    }

    impl<T, const N: usize> Sender<T, N> {
        /// Try to send a value. Returns `Err(Full(value))` if the channel is full.
        pub fn try_send(&self, value: T) -> Result<(), Full<T>> {
            self.inner.try_send(value)
        }

        /// Returns the number of items currently in the channel.
        pub fn len(&self) -> usize {
            self.inner.len()
        }

        /// Returns `true` if the channel is empty.
        pub fn is_empty(&self) -> bool {
            self.inner.is_empty()
        }

        /// Returns `true` if the channel is full.
        pub fn is_full(&self) -> bool {
            self.inner.is_full()
        }
    }

    impl<T, const N: usize> Clone for Sender<T, N> {
        fn clone(&self) -> Self {
            Self {
                inner: self.inner.clone(),
            }
        }
    }

    impl<T, const N: usize> Receiver<T, N> {
        /// Try to receive a value. Returns `None` if the channel is empty.
        pub fn try_recv(&self) -> Option<T> {
            self.inner.try_recv()
        }

        /// Returns the number of items currently in the channel.
        pub fn len(&self) -> usize {
            self.inner.len()
        }

        /// Returns `true` if the channel is empty.
        pub fn is_empty(&self) -> bool {
            self.inner.is_empty()
        }

        /// Returns `true` if the channel is full.
        pub fn is_full(&self) -> bool {
            self.inner.is_full()
        }
    }

    impl<T, const N: usize> Clone for Receiver<T, N> {
        fn clone(&self) -> Self {
            Self {
                inner: self.inner.clone(),
            }
        }
    }

    impl<T, const N: usize> core::fmt::Debug for Sender<T, N> {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("Sender")
                .field("len", &self.len())
                .field("capacity", &N)
                .finish()
        }
    }

    impl<T, const N: usize> core::fmt::Debug for Receiver<T, N> {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("Receiver")
                .field("len", &self.len())
                .field("capacity", &N)
                .finish()
        }
    }

    /// Create a bounded MPMC channel with capacity `N`.
    ///
    /// Returns a `(Sender, Receiver)` pair. Both can be cloned for
    /// multi-producer / multi-consumer patterns.
    pub fn bounded_channel<T, const N: usize>() -> (Sender<T, N>, Receiver<T, N>) {
        let inner = Arc::new(ChannelInner::new());
        (
            Sender { inner: inner.clone() },
            Receiver { inner },
        )
    }
}

/// no_std without alloc: pointer-based channel backed by caller-managed storage.
#[cfg(not(any(feature = "std", test)))]
#[allow(dead_code)]
mod shared {
    use super::*;

    /// Sending half of a bounded channel (no_std, pointer-based).
    pub struct Sender<T, const N: usize> { inner: *const ChannelInner<T, N> }
    /// Receiving half of a bounded channel (no_std, pointer-based).
    pub struct Receiver<T, const N: usize> { inner: *const ChannelInner<T, N> }

    unsafe impl<T: Send, const N: usize> Send for Sender<T, N> {}
    unsafe impl<T: Send, const N: usize> Sync for Sender<T, N> {}
    unsafe impl<T: Send, const N: usize> Send for Receiver<T, N> {}
    unsafe impl<T: Send, const N: usize> Sync for Receiver<T, N> {}

    impl<T, const N: usize> Sender<T, N> {
        /// Try to send a value. Returns `Err(Full(value))` if the channel is full.
        pub fn try_send(&self, value: T) -> Result<(), Full<T>> { unsafe { &*self.inner }.try_send(value) }
        /// Number of items in the channel.
        pub fn len(&self) -> usize { unsafe { &*self.inner }.len() }
        /// Returns `true` if the channel is empty.
        pub fn is_empty(&self) -> bool { unsafe { &*self.inner }.is_empty() }
        /// Returns `true` if the channel is full.
        pub fn is_full(&self) -> bool { unsafe { &*self.inner }.is_full() }
    }
    impl<T, const N: usize> Clone for Sender<T, N> { fn clone(&self) -> Self { Self { inner: self.inner } } }

    impl<T, const N: usize> Receiver<T, N> {
        /// Try to receive a value. Returns `None` if the channel is empty.
        pub fn try_recv(&self) -> Option<T> { unsafe { &*self.inner }.try_recv() }
        /// Number of items in the channel.
        pub fn len(&self) -> usize { unsafe { &*self.inner }.len() }
        /// Returns `true` if the channel is empty.
        pub fn is_empty(&self) -> bool { unsafe { &*self.inner }.is_empty() }
        /// Returns `true` if the channel is full.
        pub fn is_full(&self) -> bool { unsafe { &*self.inner }.is_full() }
    }
    impl<T, const N: usize> Clone for Receiver<T, N> { fn clone(&self) -> Self { Self { inner: self.inner } } }

    impl<T, const N: usize> core::fmt::Debug for Sender<T, N> {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.debug_struct("Sender").field("capacity", &N).finish() }
    }
    impl<T, const N: usize> core::fmt::Debug for Receiver<T, N> {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.debug_struct("Receiver").field("capacity", &N).finish() }
    }

    /// Create a bounded channel. Panics in no_std without alloc — use static storage.
    pub fn bounded_channel<T, const N: usize>() -> (Sender<T, N>, Receiver<T, N>) {
        panic!("bounded_channel requires alloc feature or static storage in no_std mode")
    }
}

pub use shared::{bounded_channel, Receiver, Sender};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_send_recv() {
        let (tx, rx) = bounded_channel::<u32, 4>();
        tx.try_send(1).unwrap();
        tx.try_send(2).unwrap();
        assert_eq!(rx.try_recv(), Some(1));
        assert_eq!(rx.try_recv(), Some(2));
        assert_eq!(rx.try_recv(), None);
    }

    #[test]
    fn channel_full() {
        let (tx, rx) = bounded_channel::<u32, 2>();
        tx.try_send(1).unwrap();
        tx.try_send(2).unwrap();
        assert!(tx.is_full());
        let err = tx.try_send(3).unwrap_err();
        assert_eq!(err.into_inner(), 3);
        // After receiving, can send again
        rx.try_recv().unwrap();
        tx.try_send(3).unwrap();
    }

    #[test]
    fn channel_empty() {
        let (_tx, rx) = bounded_channel::<u32, 4>();
        assert!(rx.is_empty());
        assert_eq!(rx.try_recv(), None);
    }

    #[test]
    fn channel_len() {
        let (tx, rx) = bounded_channel::<u32, 8>();
        assert_eq!(tx.len(), 0);
        tx.try_send(1).unwrap();
        tx.try_send(2).unwrap();
        assert_eq!(tx.len(), 2);
        assert_eq!(rx.len(), 2);
        rx.try_recv().unwrap();
        assert_eq!(tx.len(), 1);
    }

    #[test]
    fn channel_fifo_order() {
        let (tx, rx) = bounded_channel::<u32, 8>();
        for i in 0..5 {
            tx.try_send(i).unwrap();
        }
        for i in 0..5 {
            assert_eq!(rx.try_recv(), Some(i));
        }
    }

    #[test]
    fn channel_clone_sender() {
        let (tx, rx) = bounded_channel::<u32, 8>();
        let tx2 = tx.clone();
        tx.try_send(1).unwrap();
        tx2.try_send(2).unwrap();
        let a = rx.try_recv().unwrap();
        let b = rx.try_recv().unwrap();
        assert!((a == 1 && b == 2) || (a == 2 && b == 1));
    }

    #[test]
    fn channel_clone_receiver() {
        let (tx, rx) = bounded_channel::<u32, 8>();
        let rx2 = rx.clone();
        tx.try_send(1).unwrap();
        tx.try_send(2).unwrap();
        // Each item is received by exactly one receiver
        let a = rx.try_recv();
        let b = rx2.try_recv();
        assert!(a.is_some() || b.is_some());
    }

    #[test]
    fn channel_wrap_around() {
        let (tx, rx) = bounded_channel::<u32, 4>();
        // Fill and drain multiple times to exercise wrap-around
        for round in 0..3 {
            for i in 0..4 {
                tx.try_send(round * 4 + i).unwrap();
            }
            for i in 0..4 {
                assert_eq!(rx.try_recv(), Some(round * 4 + i));
            }
        }
    }

    #[test]
    fn channel_zero_capacity() {
        let (tx, _rx) = bounded_channel::<u32, 0>();
        assert!(tx.try_send(1).is_err());
    }

    #[test]
    fn channel_drop_with_items() {
        use core::sync::atomic::{AtomicU32, Ordering};
        static DROP_COUNT: AtomicU32 = AtomicU32::new(0);

        #[derive(Debug)]
        struct Droppable(u32);
        impl Drop for Droppable {
            fn drop(&mut self) {
                DROP_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }

        DROP_COUNT.store(0, Ordering::Relaxed);
        {
            let (tx, _rx) = bounded_channel::<Droppable, 8>();
            tx.try_send(Droppable(1)).unwrap();
            tx.try_send(Droppable(2)).unwrap();
            tx.try_send(Droppable(3)).unwrap();
            // Drop channel with 3 items still in it
        }
        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn channel_debug() {
        let (tx, rx) = bounded_channel::<u32, 8>();
        tx.try_send(1).unwrap();
        let s = alloc::format!("{:?}", tx);
        assert!(s.contains("Sender"));
        let s = alloc::format!("{:?}", rx);
        assert!(s.contains("Receiver"));
    }

    #[test]
    fn full_into_inner() {
        let f = Full(42u32);
        assert_eq!(f.into_inner(), 42);
    }
}

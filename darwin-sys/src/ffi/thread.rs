//! Raw pthread FFI — create, join, TLS, mutex, condition variables.

use crate::ffi::types::*;

/// Thread entry-point type: `void *(*)(void *)`.
pub type ThreadStart = unsafe extern "C" fn(*mut c_void) -> *mut c_void;

#[link(name = "System")]
extern "C" {
    // ── Thread lifecycle ─────────────────────────────────────────────
    pub fn pthread_create(
        thread:  *mut PthreadT,
        attr:    *const PthreadAttrT,
        start:   ThreadStart,
        arg:     *mut c_void,
    ) -> c_int;
    pub fn pthread_join(thread: PthreadT, retval: *mut *mut c_void) -> c_int;
    pub fn pthread_detach(thread: PthreadT) -> c_int;
    pub fn pthread_self() -> PthreadT;
    pub fn pthread_equal(t1: PthreadT, t2: PthreadT) -> c_int;
    pub fn pthread_exit(retval: *mut c_void) -> !;
    pub fn pthread_cancel(thread: PthreadT) -> c_int;

    // ── Thread attributes ────────────────────────────────────────────
    pub fn pthread_attr_init(attr: *mut PthreadAttrT) -> c_int;
    pub fn pthread_attr_destroy(attr: *mut PthreadAttrT) -> c_int;
    pub fn pthread_attr_setdetachstate(attr: *mut PthreadAttrT, state: c_int) -> c_int;
    pub fn pthread_attr_getdetachstate(attr: *const PthreadAttrT, state: *mut c_int) -> c_int;
    pub fn pthread_attr_setstacksize(attr: *mut PthreadAttrT, stacksize: size_t) -> c_int;
    pub fn pthread_attr_getstacksize(attr: *const PthreadAttrT, stacksize: *mut size_t) -> c_int;

    // ── Thread-local storage ─────────────────────────────────────────
    pub fn pthread_key_create(key: *mut PthreadKeyT, destructor: Option<unsafe extern "C" fn(*mut c_void)>) -> c_int;
    pub fn pthread_key_delete(key: PthreadKeyT) -> c_int;
    pub fn pthread_getspecific(key: PthreadKeyT) -> *mut c_void;
    pub fn pthread_setspecific(key: PthreadKeyT, value: *const c_void) -> c_int;

    // ── Mutex ────────────────────────────────────────────────────────
    pub fn pthread_mutex_init(mutex: *mut PthreadMutexT, attr: *const PthreadMutexattrT) -> c_int;
    pub fn pthread_mutex_destroy(mutex: *mut PthreadMutexT) -> c_int;
    pub fn pthread_mutex_lock(mutex: *mut PthreadMutexT) -> c_int;
    pub fn pthread_mutex_trylock(mutex: *mut PthreadMutexT) -> c_int;
    pub fn pthread_mutex_unlock(mutex: *mut PthreadMutexT) -> c_int;

    pub fn pthread_mutexattr_init(attr: *mut PthreadMutexattrT) -> c_int;
    pub fn pthread_mutexattr_destroy(attr: *mut PthreadMutexattrT) -> c_int;
    pub fn pthread_mutexattr_settype(attr: *mut PthreadMutexattrT, kind: c_int) -> c_int;

    // ── Condition variable ───────────────────────────────────────────
    pub fn pthread_cond_init(cond: *mut PthreadCondT, attr: *const PthreadCondattrT) -> c_int;
    pub fn pthread_cond_destroy(cond: *mut PthreadCondT) -> c_int;
    pub fn pthread_cond_wait(cond: *mut PthreadCondT, mutex: *mut PthreadMutexT) -> c_int;
    pub fn pthread_cond_timedwait(
        cond:    *mut PthreadCondT,
        mutex:   *mut PthreadMutexT,
        abstime: *const Timespec,
    ) -> c_int;
    pub fn pthread_cond_signal(cond: *mut PthreadCondT) -> c_int;
    pub fn pthread_cond_broadcast(cond: *mut PthreadCondT) -> c_int;

    pub fn pthread_condattr_init(attr: *mut PthreadCondattrT) -> c_int;
    pub fn pthread_condattr_destroy(attr: *mut PthreadCondattrT) -> c_int;

    // ── Read-write lock ──────────────────────────────────────────────
    pub fn pthread_rwlock_init(
        rwlock: *mut PthreadRwlockT,
        attr:   *const c_void,
    ) -> c_int;
    pub fn pthread_rwlock_destroy(rwlock: *mut PthreadRwlockT) -> c_int;
    pub fn pthread_rwlock_rdlock(rwlock: *mut PthreadRwlockT) -> c_int;
    pub fn pthread_rwlock_tryrdlock(rwlock: *mut PthreadRwlockT) -> c_int;
    pub fn pthread_rwlock_wrlock(rwlock: *mut PthreadRwlockT) -> c_int;
    pub fn pthread_rwlock_trywrlock(rwlock: *mut PthreadRwlockT) -> c_int;
    pub fn pthread_rwlock_unlock(rwlock: *mut PthreadRwlockT) -> c_int;

    // ── Once ─────────────────────────────────────────────────────────
    pub fn pthread_once(
        once_control: *mut PthreadOnceT,
        init_routine: unsafe extern "C" fn(),
    ) -> c_int;
}

/// pthread_once_t on macOS: { long __sig; char __opaque[8] } = 16 bytes.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PthreadOnceT {
    pub __sig:    i64,
    pub __opaque: [u8; 8],
}

impl PthreadOnceT {
    /// The only valid static initializer for pthread_once_t.
    pub const INIT: Self = Self { __sig: 0x30B1BCBA, __opaque: [0; 8] };
}

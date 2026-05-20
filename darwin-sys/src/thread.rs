//! Safe pthread wrappers: Thread lifecycle, TLS keys, mutex, condvar, rwlock.

use crate::ffi::thread as raw;
use crate::ffi::types::*;
use crate::error::{OsError, Result};

// ── Thread ──────────────────────────────────────────────────────────────

/// An owned joinable thread handle.
pub struct JoinHandle(PthreadT);

/// Spawn a thread running `f`.  Returns a joinable handle.
///
/// # Safety
/// `f` must not capture references with lifetimes shorter than the returned
/// handle — the thread will outlive the current stack frame until joined.
pub unsafe fn spawn_raw(f: raw::ThreadStart, arg: *mut c_void) -> Result<JoinHandle> {
    let mut t: PthreadT = core::mem::zeroed();
    let r = raw::pthread_create(&mut t, core::ptr::null(), f, arg);
    if r != 0 { Err(OsError(r)) } else { Ok(JoinHandle(t)) }
}

impl JoinHandle {
    /// Block until the thread finishes.  Returns the thread's return value.
    pub fn join(self) -> Result<*mut c_void> {
        let mut retval: *mut c_void = core::ptr::null_mut();
        let r = unsafe { raw::pthread_join(self.0, &mut retval) };
        core::mem::forget(self);
        if r != 0 { Err(OsError(r)) } else { Ok(retval) }
    }

    /// Detach the thread; it will clean up automatically on exit.
    pub fn detach(self) -> Result<()> {
        let r = unsafe { raw::pthread_detach(self.0) };
        core::mem::forget(self);
        if r != 0 { Err(OsError(r)) } else { Ok(()) }
    }
}

impl Drop for JoinHandle {
    fn drop(&mut self) {
        // Detach to avoid a resource leak if the handle is dropped without joining.
        unsafe { raw::pthread_detach(self.0); }
    }
}

#[inline] pub fn current() -> PthreadT { unsafe { raw::pthread_self() } }

// ── TLS key ─────────────────────────────────────────────────────────────

/// An owned thread-local storage key.
pub struct TlsKey(PthreadKeyT);

impl TlsKey {
    /// Create a new TLS key with an optional destructor called on thread exit.
    pub fn new(dtor: Option<unsafe extern "C" fn(*mut c_void)>) -> Result<Self> {
        let mut key: PthreadKeyT = 0;
        let r = unsafe { raw::pthread_key_create(&mut key, dtor) };
        if r != 0 { Err(OsError(r)) } else { Ok(Self(key)) }
    }

    #[inline]
    pub fn get(&self) -> *mut c_void {
        unsafe { raw::pthread_getspecific(self.0) }
    }

    #[inline]
    pub fn set(&self, value: *const c_void) -> Result<()> {
        let r = unsafe { raw::pthread_setspecific(self.0, value) };
        if r != 0 { Err(OsError(r)) } else { Ok(()) }
    }
}

impl Drop for TlsKey {
    fn drop(&mut self) { unsafe { raw::pthread_key_delete(self.0); } }
}

// ── Mutex ────────────────────────────────────────────────────────────────

/// An owned pthread mutex.
pub struct Mutex(PthreadMutexT);

impl Mutex {
    /// Create a default (non-recursive) mutex.
    pub fn new() -> Result<Self> {
        let mut m: PthreadMutexT = unsafe { core::mem::zeroed() };
        let r = unsafe { raw::pthread_mutex_init(&mut m, core::ptr::null()) };
        if r != 0 { Err(OsError(r)) } else { Ok(Self(m)) }
    }

    pub fn lock(&mut self) -> Result<MutexGuard<'_>> {
        let r = unsafe { raw::pthread_mutex_lock(&mut self.0) };
        if r != 0 { Err(OsError(r)) } else { Ok(MutexGuard(self)) }
    }

    pub fn try_lock(&mut self) -> Result<MutexGuard<'_>> {
        let r = unsafe { raw::pthread_mutex_trylock(&mut self.0) };
        if r != 0 { Err(OsError(r)) } else { Ok(MutexGuard(self)) }
    }

    pub(crate) fn unlock_raw(&mut self) -> Result<()> {
        let r = unsafe { raw::pthread_mutex_unlock(&mut self.0) };
        if r != 0 { Err(OsError(r)) } else { Ok(()) }
    }

    pub(crate) fn raw_ptr(&mut self) -> *mut PthreadMutexT { &mut self.0 }
}

impl Drop for Mutex {
    fn drop(&mut self) { unsafe { raw::pthread_mutex_destroy(&mut self.0); } }
}

pub struct MutexGuard<'a>(&'a mut Mutex);

impl Drop for MutexGuard<'_> {
    fn drop(&mut self) { let _ = self.0.unlock_raw(); }
}

// ── Condvar ──────────────────────────────────────────────────────────────

/// An owned pthread condition variable.
pub struct Condvar(PthreadCondT);

impl Condvar {
    pub fn new() -> Result<Self> {
        let mut c: PthreadCondT = unsafe { core::mem::zeroed() };
        let r = unsafe { raw::pthread_cond_init(&mut c, core::ptr::null()) };
        if r != 0 { Err(OsError(r)) } else { Ok(Self(c)) }
    }

    pub fn wait(&mut self, guard: &mut MutexGuard<'_>) -> Result<()> {
        let r = unsafe { raw::pthread_cond_wait(&mut self.0, guard.0.raw_ptr()) };
        if r != 0 { Err(OsError(r)) } else { Ok(()) }
    }

    pub fn wait_timeout(&mut self, guard: &mut MutexGuard<'_>, abstime: &Timespec) -> Result<()> {
        let r = unsafe { raw::pthread_cond_timedwait(&mut self.0, guard.0.raw_ptr(), abstime) };
        if r != 0 { Err(OsError(r)) } else { Ok(()) }
    }

    pub fn signal(&mut self) -> Result<()> {
        let r = unsafe { raw::pthread_cond_signal(&mut self.0) };
        if r != 0 { Err(OsError(r)) } else { Ok(()) }
    }

    pub fn broadcast(&mut self) -> Result<()> {
        let r = unsafe { raw::pthread_cond_broadcast(&mut self.0) };
        if r != 0 { Err(OsError(r)) } else { Ok(()) }
    }
}

impl Drop for Condvar {
    fn drop(&mut self) { unsafe { raw::pthread_cond_destroy(&mut self.0); } }
}

// ── RwLock ───────────────────────────────────────────────────────────────

/// An owned pthread read-write lock.
pub struct RwLock(PthreadRwlockT);

impl RwLock {
    pub fn new() -> Result<Self> {
        let mut rw: PthreadRwlockT = unsafe { core::mem::zeroed() };
        let r = unsafe { raw::pthread_rwlock_init(&mut rw, core::ptr::null()) };
        if r != 0 { Err(OsError(r)) } else { Ok(Self(rw)) }
    }

    pub fn read(&mut self) -> Result<RwLockReadGuard<'_>> {
        let r = unsafe { raw::pthread_rwlock_rdlock(&mut self.0) };
        if r != 0 { Err(OsError(r)) } else { Ok(RwLockReadGuard(self)) }
    }

    pub fn try_read(&mut self) -> Result<RwLockReadGuard<'_>> {
        let r = unsafe { raw::pthread_rwlock_tryrdlock(&mut self.0) };
        if r != 0 { Err(OsError(r)) } else { Ok(RwLockReadGuard(self)) }
    }

    pub fn write(&mut self) -> Result<RwLockWriteGuard<'_>> {
        let r = unsafe { raw::pthread_rwlock_wrlock(&mut self.0) };
        if r != 0 { Err(OsError(r)) } else { Ok(RwLockWriteGuard(self)) }
    }

    pub fn try_write(&mut self) -> Result<RwLockWriteGuard<'_>> {
        let r = unsafe { raw::pthread_rwlock_trywrlock(&mut self.0) };
        if r != 0 { Err(OsError(r)) } else { Ok(RwLockWriteGuard(self)) }
    }

    fn unlock_raw(&mut self) { unsafe { raw::pthread_rwlock_unlock(&mut self.0); } }
}

impl Drop for RwLock {
    fn drop(&mut self) { unsafe { raw::pthread_rwlock_destroy(&mut self.0); } }
}

pub struct RwLockReadGuard<'a>(&'a mut RwLock);
impl Drop for RwLockReadGuard<'_> { fn drop(&mut self) { self.0.unlock_raw(); } }

pub struct RwLockWriteGuard<'a>(&'a mut RwLock);
impl Drop for RwLockWriteGuard<'_> { fn drop(&mut self) { self.0.unlock_raw(); } }

// ── Once ────────────────────────────────────────────────────────────────

pub use raw::PthreadOnceT;

/// Run `f` exactly once across all threads.
///
/// # Safety
/// `once` must be initialized with `PthreadOnceT::INIT`.
pub unsafe fn call_once(once: &mut PthreadOnceT, f: unsafe extern "C" fn()) -> Result<()> {
    let r = raw::pthread_once(once, f);
    if r != 0 { Err(OsError(r)) } else { Ok(()) }
}

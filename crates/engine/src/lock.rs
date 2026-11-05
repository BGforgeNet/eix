//! A spin lock for the engine's short critical sections, without std.
#![expect(unsafe_code, reason = "handing out the locked value")]

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

/// Guards `T` for sections of a few system calls at most; waiters spin.
pub(crate) struct SpinLock<T> {
    taken: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: `value` is reached only through `with`, while `taken` is held.
unsafe impl<T: Send> Sync for SpinLock<T> {}

/// Releases the lock when dropped, so a panic unwinding out of a test does not leave it taken.
struct Release<'a>(&'a AtomicBool);

impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl<T> SpinLock<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self {
            taken: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    /// Runs `f` on the value while holding the lock.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        while self
            .taken
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        let _release = Release(&self.taken);
        // SAFETY: `taken` is held until `_release` drops, so this is the only reference to the value.
        f(unsafe { &mut *self.value.get() })
    }

    /// Runs `f` on the value if the lock is free; none when it is taken. For a crash handler, which must not wait on
    /// a lock the crashing thread may hold.
    pub(crate) fn try_with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.taken
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()?;
        let _release = Release(&self.taken);
        // SAFETY: as in `with`.
        Some(f(unsafe { &mut *self.value.get() }))
    }
}

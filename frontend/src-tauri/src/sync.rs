//! Poison-tolerant locking for recording, engine and save state.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks a mutex, taking over the guard if an earlier holder panicked.
///
/// This is the crate's existing `unwrap_or_else(PoisonError::into_inner)`
/// recovery under one name. Use it only for state that stays consistent when a
/// holder panics part-way, because each update is a single replacement, push or
/// counter step. A panicking callback or task then costs that one operation
/// instead of turning every later recording command into a panic.
pub(crate) trait LockRecovering<T: ?Sized> {
    fn lock_recovering(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> LockRecovering<T> for Mutex<T> {
    fn lock_recovering(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

//! Small helpers shared across modules.

use std::sync::{Mutex, MutexGuard};

/// Locks a mutex, recovering the data if another thread panicked while holding it. Every
/// mutex in this app guards state that is consistent between operations, so a poisoned lock
/// is not a reason to take the whole application down.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

//! One-line managed-state locking with uniform poison messages.
//!
//! Every Tauri command below locks managed `Mutex` state the same way; this
//! helper collapses the three-line `.lock().map_err(...)` boilerplate while
//! preserving the exact historical messages. It accepts anything that
//! dereferences to a `Mutex` (`tauri::State<Mutex<T>>`, `Arc<Mutex<T>>`), so
//! field locks like `capture.ingestion_guard` convert unchanged.

use std::ops::Deref;
use std::sync::{Mutex, MutexGuard};

pub fn lock_state<'a, M, T>(state: &'a M, name: &'static str) -> Result<MutexGuard<'a, T>, String>
where
    M: Deref<Target = Mutex<T>>,
{
    // Explicit deref coercion (rather than relying on method-call autoderef)
    // so the guard lifetime ties to the caller's borrow.
    let mutex: &'a Mutex<T> = state;
    mutex.lock().map_err(|_| format!("{name} lock is poisoned"))
}

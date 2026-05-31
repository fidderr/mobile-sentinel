//! Test-only mutex serialising access to the process-global Recipe registry.
//!
//! Multiple registry-touching tests across the recipes module share this
//! mutex so they run sequentially, even under cargo's per-binary thread
//! pool.

#![cfg(test)]

use std::sync::{Mutex, MutexGuard};

use crate::recipes::registry::recipe_registry;

static GLOBAL_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the global registry mutex and clear the registry so each
/// test starts from a known empty state.
pub fn lock_and_clear<'a>() -> MutexGuard<'a, ()> {
    let g = GLOBAL_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    recipe_registry().clear();
    g
}

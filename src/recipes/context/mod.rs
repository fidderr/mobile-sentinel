//! Recipe Context store — the recipe layer's specialization of the generic
//! [`crate::state_store::StateStore`].
//!
//! `ContextStore` is `StateStore<ContextRecord>`: the same atomic, per-id,
//! revisioned JSON store, with the recipe-specific [`RecipeContext`] payload.
//! Both MAIN and `:sentinel` read the same files on disk; the filesystem is
//! the contract.

pub mod schema;

use crate::state_store::StateStore;

pub use crate::state_store::{Revision, StoreError};
pub use schema::{AlarmClassContext, ContextRecord, RecipeContext};

/// Atomic, per-id store for recipe instance state.
///
/// A thin specialization of [`StateStore`] over [`ContextRecord`]. All the
/// store mechanics (atomic write, per-id mutex, revisions, enumerate) live
/// in `state_store`; this alias just fixes the payload type.
pub type ContextStore = StateStore<ContextRecord>;

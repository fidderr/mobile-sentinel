//! Core handle and identifier types for mobile-sentinel.

use serde::{Deserialize, Serialize};

/// Unique identifier for a stored instance (e.g., one alarm, one timer
/// session). Used as the primary key into a `StateStore` (and the recipe
/// layer's `ContextStore`).
///
/// `InstanceId` is opaque — consumers create them via UUIDs and never parse
/// the contents.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceId(pub String);

impl InstanceId {
    /// Create a new InstanceId wrapping the given string.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Borrow the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for InstanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for InstanceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for InstanceId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// Handle for active audio playback.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlaybackHandle(pub(crate) u64);

impl PlaybackHandle {
    /// Get the raw handle ID (for storing/restoring across boundaries).
    pub fn id(&self) -> u64 {
        self.0
    }

    /// Reconstruct a handle from a raw ID.
    pub fn from_id(id: u64) -> Self {
        Self(id)
    }
}

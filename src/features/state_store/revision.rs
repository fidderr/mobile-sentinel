//! Monotonic revision counter for ContextRecords.
//!
//! Every successful write increments `revision`. Used by audit logs and
//! tests to detect lost updates and verify last-writer-wins ordering.

use serde::{Deserialize, Serialize};

/// Monotonically-increasing revision number for a ContextRecord.
///
/// Each successful `ContextStore::write` produces a record whose revision
/// is exactly one greater than the previously persisted revision.
/// New records start at `Revision(1)`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct Revision(pub u64);

impl Revision {
    /// Initial revision for a freshly-created record.
    pub const INITIAL: Revision = Revision(1);

    /// Return the next revision (current + 1).
    pub fn next(self) -> Revision {
        Revision(self.0.saturating_add(1))
    }

    /// Raw u64 value.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for Revision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rev{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_is_one() {
        assert_eq!(Revision::INITIAL, Revision(1));
    }

    #[test]
    fn next_increments_by_one() {
        assert_eq!(Revision(0).next(), Revision(1));
        assert_eq!(Revision(1).next(), Revision(2));
        assert_eq!(Revision(42).next(), Revision(43));
    }

    #[test]
    fn next_saturates_at_max() {
        assert_eq!(Revision(u64::MAX).next(), Revision(u64::MAX));
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(Revision(1) < Revision(2));
        assert!(Revision(100) > Revision(99));
    }

    #[test]
    fn round_trips_as_transparent_number() {
        let r = Revision(42);
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, "42");
        let back: Revision = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}

//! Trigger catalogue.
//!
//! Every entry point that drives a Recipe state transition is enumerated
//! here. Triggers are typed and serializable; cross-process dispatch
//! always goes through `Trigger::parse_tag` / `Trigger::as_str`.
//!
//! The recipe layer's dispatcher ([`crate::recipes::dispatch_trigger`]) routes
//! these to the affected Recipe; the actual platform side effects a Recipe
//! performs are issued directly against the [`crate::firing::FiringSink`].
//! There is no intermediate "Plan" / "Action" layer — Recipes call sink
//! methods.

use serde::{Deserialize, Serialize};

/// Triggers — every Recipe state-transition entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// Primary fire event (scheduled time reached or snooze elapsed).
    Fire,
    /// User-initiated snooze.
    Snooze,
    /// User-initiated dismiss.
    Dismiss,
    /// Challenge solved — unlocks dismiss.
    Solve,
    /// User edited the instance while scheduled.
    Edit,
    /// Arm a new OS-level wake.
    Schedule,
    /// Audio pause (e.g., phone call started).
    Pause,
    /// Audio resume (e.g., phone call ended).
    Resume,
}

impl Trigger {
    /// Stable lower-snake-case identifier — matches the JSON encoding.
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Fire => "fire",
            Trigger::Snooze => "snooze",
            Trigger::Dismiss => "dismiss",
            Trigger::Solve => "solve",
            Trigger::Edit => "edit",
            Trigger::Schedule => "schedule",
            Trigger::Pause => "pause",
            Trigger::Resume => "resume",
        }
    }

    /// Parse from the stable string form. Returns `None` for unknown
    /// values. The cross-process job-file contract uses this on the
    /// receive side.
    pub fn parse_tag(s: &str) -> Option<Self> {
        Some(match s {
            "fire" => Trigger::Fire,
            "snooze" => Trigger::Snooze,
            "dismiss" => Trigger::Dismiss,
            "solve" => Trigger::Solve,
            "edit" => Trigger::Edit,
            "schedule" => Trigger::Schedule,
            "pause" => Trigger::Pause,
            "resume" => Trigger::Resume,
            _ => return None,
        })
    }

    /// Every variant in declaration order — useful for iteration in tests.
    pub const ALL: &'static [Trigger] = &[
        Trigger::Fire,
        Trigger::Snooze,
        Trigger::Dismiss,
        Trigger::Solve,
        Trigger::Edit,
        Trigger::Schedule,
        Trigger::Pause,
        Trigger::Resume,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        for t in Trigger::ALL {
            let json = serde_json::to_string(t).unwrap();
            let back: Trigger = serde_json::from_str(&json).unwrap();
            assert_eq!(*t, back);
        }
    }

    #[test]
    fn as_str_matches_serde_form() {
        for t in Trigger::ALL {
            let json = serde_json::to_string(t).unwrap();
            assert_eq!(json, format!("\"{}\"", t.as_str()));
        }
    }

    #[test]
    fn from_str_round_trips() {
        for t in Trigger::ALL {
            assert_eq!(Trigger::parse_tag(t.as_str()), Some(*t));
        }
    }

    #[test]
    fn from_str_returns_none_for_unknown() {
        assert!(Trigger::parse_tag("not_a_trigger").is_none());
        assert!(Trigger::parse_tag("").is_none());
    }

    #[test]
    fn all_array_has_eight_variants() {
        assert_eq!(Trigger::ALL.len(), 8);
    }
}

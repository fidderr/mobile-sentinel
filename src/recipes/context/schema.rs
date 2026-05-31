//! Per-Recipe Context schema types.
//!
//! Each Recipe defines its own typed Context struct (e.g. `AlarmClassContext`).
//! All Recipe contexts are wrapped in [`RecipeContext`] (a tagged enum) and
//! persisted inside a [`ContextRecord`] envelope that carries the session
//! state and the monotonic [`super::Revision`].
//!
//! # Adding a new Recipe
//!
//! 1. Define a `<Recipe>Context` struct with all configurable settings.
//! 2. Add a variant to [`RecipeContext`] tagged with the recipe identifier.
//! 3. The Recipe's `context_schema_name()` must match the variant tag.

use serde::{Deserialize, Serialize};

use crate::state_store::Revision;
use crate::types::InstanceId;

// Re-export the snooze types so consumers can import everything
// alarm-context-shaped from a single module path.
pub use crate::recipes::snooze::{EscalationPolicy, SnoozePolicy};

/// The lifecycle state of a Recipe instance, persisted alongside the Context.
///
/// Mirrors the canonical state machine in `requirements.md`. Each variant
/// carries enough timestamp/counter state to reconstruct a session after
/// process death or reboot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionState {
    /// No active session. Instance exists in the store but is not scheduled
    /// or firing. Used after dismiss for one-shot Recipes.
    Idle,
    /// Scheduled to fire at `next_fire_unix_ms`.
    Scheduled { next_fire_unix_ms: i64 },
    /// Actively firing (sound playing, FGS running, UI shown).
    Firing { fired_at_unix_ms: i64 },
    /// Snoozed; will re-fire at `next_fire_unix_ms`. The `snooze_count`
    /// monotonically increases — see Snooze Policy property test.
    Snoozed {
        snooze_count: u32,
        next_fire_unix_ms: i64,
    },
    /// Dismissed at `dismissed_at_unix_ms`. Recurring Recipes transition
    /// back to `Scheduled` from here; one-shot Recipes go to `Idle`.
    Dismissed { dismissed_at_unix_ms: i64 },
}

impl SessionState {
    /// Stable, lower-snake-case tag identifying this state for audit logs.
    pub fn tag(&self) -> &'static str {
        match self {
            SessionState::Idle => "idle",
            SessionState::Scheduled { .. } => "scheduled",
            SessionState::Firing { .. } => "firing",
            SessionState::Snoozed { .. } => "snoozed",
            SessionState::Dismissed { .. } => "dismissed",
        }
    }
}

/// A Recipe-specific Context tagged by recipe type.
///
/// Only Recipes shipped by mobile-sentinel are encoded here — consumers
/// adding custom Recipes use the `Custom { payload_json }` variant for
/// forward compatibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "recipe_type", rename_all = "snake_case")]
pub enum RecipeContext {
    AlarmClass(AlarmClassContext),
    /// Escape hatch for consumer-defined Recipes. The `recipe_type` tag in
    /// the JSON wrapper identifies the Recipe; `payload_json` is the
    /// consumer's serialized struct.
    Custom {
        payload_json: String,
    },
}

impl RecipeContext {
    /// Stable tag identifying the Recipe type — matches the
    /// `Recipe::recipe_type()` value.
    pub fn recipe_type(&self) -> &'static str {
        match self {
            RecipeContext::AlarmClass(_) => "alarm_class",
            RecipeContext::Custom { .. } => "custom",
        }
    }
}

// ---------------------------------------------------------------------------
// AlarmClass — generic scheduled-fire Recipe with snooze + dismiss + sound
// ---------------------------------------------------------------------------

/// AlarmClass instance settings.
///
/// Schedule, sound, snooze policy, and challenge gate config are persisted
/// here so every Trigger handler reads the same values
/// (LoadContext-on-every-trigger invariant).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlarmClassContext {
    /// Display label.
    pub label: String,
    /// When the alarm should fire (one-time, weekdays, monthly, cron).
    pub schedule: ScheduleSpec,
    /// IANA time zone the schedule is interpreted in (e.g. `"Europe/Amsterdam"`).
    pub time_zone: String,
    /// Configured sound at fire time.
    pub sound_id: SoundIdSpec,
    /// Snooze behavior config.
    pub snooze_policy: SnoozePolicy,
    /// Optional challenge gate descriptors (consumer-interpreted).
    pub challenges: Vec<ChallengeSpec>,
    /// Whether vibration should accompany sound playback.
    pub vibration_enabled: bool,
    /// Optional per-alarm vibration waveform (alternating wait/vibrate ms),
    /// looped while firing. `None` means "use the consumer's configured
    /// default" (`AlarmClassConfig::vibration_pattern`). This is the most
    /// specific level of the three-level fallback (SDK default → app config
    /// override → this per-alarm override). `#[serde(default)]` keeps older
    /// persisted records (written before this field existed) loadable.
    #[serde(default)]
    pub vibration_pattern: Option<Vec<i64>>,
    /// Whether kiosk-mode lock-task is engaged on fire.
    pub kiosk_mode: bool,
    /// Whether the firing notification channel requests bypass-DND.
    pub bypass_dnd: bool,
    /// Tracks how many snoozes the user has used in the current session;
    /// reset to 0 on dismiss or after a successful re-arm cycle.
    pub snooze_count: u32,
    /// True when the consumer-supplied challenge gate hook has signalled
    /// completion within the current Firing session. Reset on dismiss.
    pub challenges_solved: bool,
}

/// Schedule specification — a serializable description of when fires occur.
///
/// The Recurrence Engine consumes this and produces `next_fire_unix_ms`
/// values in the [`SessionState::Scheduled`] variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleSpec {
    /// Single fire at a specific local date and time-of-day.
    OneTime {
        /// `YYYY-MM-DD`.
        date: String,
        /// Local hour 0..=23.
        hour: u8,
        /// Local minute 0..=59.
        minute: u8,
    },
    /// Recurring fires on selected weekdays at a fixed time-of-day.
    Weekdays {
        /// Bitmask: bit 0 = Mon, bit 6 = Sun.
        days_mask: u8,
        hour: u8,
        minute: u8,
    },
    /// Recurring on a specific day of each month.
    Monthly {
        /// 1..=31, or 0 for last-day-of-month semantics.
        day_of_month: u8,
        hour: u8,
        minute: u8,
    },
    /// Cron-style expression (5-field standard).
    Cron { expression: String },
}

/// Stable identifier for a configured sound.
///
/// At fire time the Sound Library resolves a [`SoundIdSpec`] to a playable
/// URI, falling back to the system default if the referenced sound is
/// missing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SoundIdSpec {
    /// Bundled APK asset under the consumer's sounds directory.
    /// Identifier is the file stem (e.g. `"happy"`).
    Bundled(String),
    /// User-imported sound, identified by an opaque token (typically UUID).
    Custom(String),
    /// Platform default alarm sound.
    SystemDefault,
    /// Silent — vibration only.
    Silent,
}

/// Opaque challenge descriptor — consumer-interpreted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChallengeSpec {
    /// Challenge type identifier (consumer-defined string).
    pub challenge_type: String,
    /// Consumer-interpreted difficulty.
    pub difficulty: u8,
    /// Free-form JSON config for this challenge.
    pub config: serde_json::Value,
}

// ---------------------------------------------------------------------------
// ContextRecord — the persisted envelope
// ---------------------------------------------------------------------------

/// Persisted envelope for a Recipe instance.
///
/// One JSON file per instance under `context/<package>/<instance_id>.json`.
/// Atomic writes (write-temp + rename) plus a per-id mutex guarantee that
/// readers observe either the pre-write or post-write Context, never a
/// torn read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextRecord {
    /// Instance identifier (matches the file name without `.json`).
    pub id: InstanceId,
    /// Monotonically-increasing revision; bumped on every successful write.
    pub revision: Revision,
    /// Lifecycle state.
    pub state: SessionState,
    /// Recipe-specific configured settings (typed).
    pub context: RecipeContext,
}

impl ContextRecord {
    /// Construct a freshly-created record at [`Revision::INITIAL`] in
    /// [`SessionState::Idle`].
    pub fn new(id: InstanceId, context: RecipeContext) -> Self {
        Self {
            id,
            revision: Revision::INITIAL,
            state: SessionState::Idle,
            context,
        }
    }

    /// Convenience — the [`RecipeContext::recipe_type`] of the inner context.
    pub fn recipe_type(&self) -> &'static str {
        self.context.recipe_type()
    }
}

impl crate::state_store::Stateful for ContextRecord {
    fn instance_id(&self) -> &InstanceId {
        &self.id
    }
    fn revision(&self) -> Revision {
        self.revision
    }
    fn with_revision(mut self, revision: Revision) -> Self {
        self.revision = revision;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_state_tag_matches_variant() {
        assert_eq!(SessionState::Idle.tag(), "idle");
        assert_eq!(
            SessionState::Scheduled {
                next_fire_unix_ms: 0
            }
            .tag(),
            "scheduled"
        );
        assert_eq!(
            SessionState::Firing {
                fired_at_unix_ms: 0
            }
            .tag(),
            "firing"
        );
        assert_eq!(
            SessionState::Snoozed {
                snooze_count: 0,
                next_fire_unix_ms: 0
            }
            .tag(),
            "snoozed"
        );
        assert_eq!(
            SessionState::Dismissed {
                dismissed_at_unix_ms: 0
            }
            .tag(),
            "dismissed"
        );
    }

    #[test]
    fn recipe_context_tag_matches_variant() {
        let ctx = RecipeContext::AlarmClass(sample_alarm_class());
        assert_eq!(ctx.recipe_type(), "alarm_class");
    }

    #[test]
    fn context_record_round_trips_through_json() {
        let record = ContextRecord::new(
            InstanceId::new("abc"),
            RecipeContext::AlarmClass(sample_alarm_class()),
        );
        let json = serde_json::to_string(&record).unwrap();
        let back: ContextRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record, back);
    }

    #[test]
    fn new_record_starts_at_initial_revision_idle() {
        let record = ContextRecord::new(
            InstanceId::new("x"),
            RecipeContext::AlarmClass(sample_alarm_class()),
        );
        assert_eq!(record.revision, Revision::INITIAL);
        assert_eq!(record.state, SessionState::Idle);
    }

    #[test]
    fn schedule_spec_one_time_round_trips() {
        let s = ScheduleSpec::OneTime {
            date: "2026-06-01".into(),
            hour: 7,
            minute: 30,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"type\":\"one_time\""));
        let back: ScheduleSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn sound_id_round_trips() {
        for s in [
            SoundIdSpec::Bundled("happy".into()),
            SoundIdSpec::Custom("uuid-1".into()),
            SoundIdSpec::SystemDefault,
            SoundIdSpec::Silent,
        ] {
            let json = serde_json::to_string(&s).unwrap();
            let back: SoundIdSpec = serde_json::from_str(&json).unwrap();
            assert_eq!(s, back);
        }
    }

    #[test]
    fn instance_id_round_trips_as_transparent_string() {
        let id = InstanceId::new("alarm-42");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"alarm-42\"");
        let back: InstanceId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    fn sample_alarm_class() -> AlarmClassContext {
        AlarmClassContext {
            label: "Wake".into(),
            schedule: ScheduleSpec::Weekdays {
                days_mask: 0b0011111,
                hour: 7,
                minute: 0,
            },
            time_zone: "UTC".into(),
            sound_id: SoundIdSpec::SystemDefault,
            snooze_policy: SnoozePolicy {
                max_count: 3,
                interval_minutes: 5,
                escalation: None,
            },
            challenges: vec![],
            vibration_enabled: true,
            vibration_pattern: None,
            kiosk_mode: true,
            bypass_dnd: true,
            snooze_count: 0,
            challenges_solved: false,
        }
    }
}

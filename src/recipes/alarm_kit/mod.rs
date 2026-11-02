//! AlarmKit — the consumer-facing stateful façade over [`AlarmClass`].
//!
//! Modelled on Apple's AlarmKit and the og-idea spec: consumers create,
//! update, snooze, dismiss, and inspect alarms through a single
//! [`AlarmKit`] handle. Internally every operation goes through the
//! [`crate::recipes::context::ContextStore`] (single source of truth) plus the
//! global [`AlarmClass`] Recipe (state machine + side effects).
//!
//! ```ignore
//! let kit = mobile_sentinel::recipes::AlarmKit::install(store, sink, sound_resolver);
//! kit.create(AlarmSpec { /* ... */ })?;
//! kit.snooze(&id)?;
//! kit.dismiss(&id)?;
//! ```

/// The AlarmClass state machine that AlarmKit drives. Re-exported at the
/// recipe-layer root as `mobile_sentinel::recipes::alarm_class`.
pub mod alarm_class;

use std::sync::Arc;

use chrono::Utc;
use thiserror::Error;
use uuid::Uuid;

use crate::firing::FiringSink;
pub use crate::jobs::RingEnd;
use crate::recipes::alarm_class::{
    alarm_class_runtime, AlarmClass, AlarmClassConfig, SoundResolver,
};
use crate::recipes::context::schema::{
    AlarmClassContext, ChallengeSpec, ContextRecord, RecipeContext, ScheduleSpec, SessionState,
    SnoozePolicy, SoundIdSpec, DEFAULT_RING_LIMIT_SECS,
};
use crate::recipes::context::{ContextStore, StoreError};
use crate::recipes::dispatch::dispatch_trigger;
use crate::recipes::recipe::RecipeError;
use crate::recipes::registry::{recipe_registry, register_recipe};
use crate::recipes::trigger::Trigger;
use crate::types::InstanceId;

/// Consumer-facing alarm specification. Maps 1:1 to [`AlarmClassContext`]
/// minus the runtime state fields.
#[derive(Debug, Clone, PartialEq)]
pub struct AlarmSpec {
    pub label: String,
    pub schedule: ScheduleSpec,
    pub time_zone: String,
    pub sound_id: SoundIdSpec,
    pub snooze_policy: SnoozePolicy,
    pub challenges: Vec<ChallengeSpec>,
    pub vibration_enabled: bool,
    /// Optional per-alarm vibration waveform (alternating wait/vibrate ms).
    /// `None` → use the consumer's `AlarmClassConfig::vibration_pattern`
    /// default. Set `Some(..)` to override the pattern for THIS alarm only.
    pub vibration_pattern: Option<Vec<i64>>,
    pub kiosk_mode: bool,
    pub bypass_dnd: bool,
    /// Skip every occurrence at or before this instant (unix ms); see
    /// [`AlarmClassContext::skip_until_unix_ms`].
    pub skip_until_unix_ms: Option<i64>,
    /// Fade-in time of the alarm sound in seconds (0 = off).
    pub fade_in_secs: u16,
    /// Stop by itself after ringing this long with nobody doing anything, in
    /// seconds. `None` rings until it is dealt with.
    pub ring_limit_secs: Option<u32>,
    /// The alarm volume while this alarm rings is at least this percentage. `None`
    /// → the consumer's `AlarmClassConfig::min_volume_percent`.
    pub min_volume_percent: Option<u8>,
}

impl Default for AlarmSpec {
    fn default() -> Self {
        Self {
            label: "Alarm".into(),
            schedule: ScheduleSpec::OneTime {
                date: Utc::now().date_naive().format("%Y-%m-%d").to_string(),
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
            kiosk_mode: false,
            bypass_dnd: false,
            skip_until_unix_ms: None,
            fade_in_secs: 0,
            ring_limit_secs: Some(DEFAULT_RING_LIMIT_SECS),
            min_volume_percent: None,
        }
    }
}

impl AlarmSpec {
    fn into_context(self) -> AlarmClassContext {
        AlarmClassContext {
            label: self.label,
            schedule: self.schedule,
            time_zone: self.time_zone,
            sound_id: self.sound_id,
            snooze_policy: self.snooze_policy,
            challenges: self.challenges,
            vibration_enabled: self.vibration_enabled,
            vibration_pattern: self.vibration_pattern,
            kiosk_mode: self.kiosk_mode,
            bypass_dnd: self.bypass_dnd,
            snooze_count: 0,
            challenges_solved: false,
            skip_until_unix_ms: self.skip_until_unix_ms,
            fade_in_secs: self.fade_in_secs,
            ring_limit_secs: self.ring_limit_secs,
            min_volume_percent: self.min_volume_percent,
            first_fired_unix_ms: None,
        }
    }

    /// Reconstruct a spec from a stored context (loses `snooze_count` and
    /// `challenges_solved` runtime state — they don't belong on a spec).
    pub fn from_context(ctx: &AlarmClassContext) -> Self {
        Self {
            label: ctx.label.clone(),
            schedule: ctx.schedule.clone(),
            time_zone: ctx.time_zone.clone(),
            sound_id: ctx.sound_id.clone(),
            snooze_policy: ctx.snooze_policy.clone(),
            challenges: ctx.challenges.clone(),
            vibration_enabled: ctx.vibration_enabled,
            vibration_pattern: ctx.vibration_pattern.clone(),
            kiosk_mode: ctx.kiosk_mode,
            bypass_dnd: ctx.bypass_dnd,
            skip_until_unix_ms: ctx.skip_until_unix_ms,
            fade_in_secs: ctx.fade_in_secs,
            ring_limit_secs: ctx.ring_limit_secs,
            min_volume_percent: ctx.min_volume_percent,
        }
    }
}

/// One-time install configuration for AlarmKit. Mirrors what the
/// AlarmClass runtime needs to function.
pub struct AlarmKitConfig {
    pub store: Arc<ContextStore>,
    pub sink: Option<Arc<dyn FiringSink>>,
    pub sound_resolver: Option<Arc<dyn SoundResolver>>,
    pub alarm_class_config: AlarmClassConfig,
}

/// The AlarmKit handle. Cheap to clone — wraps `Arc<ContextStore>`.
#[derive(Clone)]
pub struct AlarmKit {
    store: Arc<ContextStore>,
}

/// Snapshot of the currently-firing AlarmClass session, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct AlarmKitSession {
    pub instance_id: InstanceId,
    /// When this ring started; a snooze ending starts a new one.
    pub fired_at_unix_ms: i64,
    /// When the first ring of this session started, across its snoozes.
    pub first_fired_unix_ms: i64,
    pub spec: AlarmSpec,
    pub snooze_count: u32,
    pub challenges_solved: bool,
}

/// What [`AlarmKit::on_startup`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupReport {
    /// Alarms that were ringing when the device rebooted. They were dismissed;
    /// the consumer may want to tell the user and clear its own per-session
    /// files.
    pub ended_by_reboot: Vec<InstanceId>,
    /// Alarms whose time passed too long ago to still ring: the phone was off,
    /// or the system never delivered them. The next occurrence, if any, is armed.
    pub missed: Vec<MissedAlarm>,
    /// One-shot alarms that have nothing left to ring. The consumer should
    /// switch them off.
    pub finished: Vec<InstanceId>,
    /// Rings the platform ended by itself while Rust was not around. They were
    /// dismissed, and the next occurrence, if any, is armed.
    pub ring_ends: Vec<(InstanceId, RingEnd)>,
}

/// An alarm that did not ring at its time. See [`StartupReport::missed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissedAlarm {
    pub id: InstanceId,
    /// When it should have rung (unix ms).
    pub due_unix_ms: i64,
}

/// What became of an alarm when it was re-armed at startup.
enum Rearmed {
    Armed,
    /// Its time has come: ring it now.
    Due,
    /// Too late to ring. `finished`: it has no later occurrence either.
    Missed {
        due_unix_ms: i64,
        finished: bool,
    },
    /// A one-shot alarm with nothing left to ring.
    Finished,
}

/// How [`AlarmKit`]'s startup rings an alarm that is late.
#[derive(Clone, Copy, PartialEq)]
enum LateRing {
    /// Right here: the app is on screen.
    Now,
    /// Through an OS alarm a second from now. Without the UI this may be the
    /// wrong moment to start a ring (Android refuses it right after boot), and an
    /// alarm being delivered is always allowed to.
    ByOsAlarm,
}

/// How long [`LateRing::ByOsAlarm`] waits.
const LATE_RING_DELAY_MS: i64 = 1_000;

/// Slack when comparing a job's active marker with the time its alarm was due.
const MARKER_SLOP_MS: i64 = 60_000;

/// [`AlarmKit::note_activity`] touches a file at most this often.
const ACTIVITY_THROTTLE_MS: i64 = 30_000;

/// Mirrors `TIMING_FILE` in `SentinelRing`, relative to the app's files dir.
const TIMING_FILE: &str = "sentinel/ringing/timing.log";

/// Mirrors `ACTIVITY_FILE` in `SentinelRing`, relative to the app's files dir.
const ACTIVITY_FILE: &str = "sentinel/ringing/activity";

/// Rings the platform ended while the app was running, until the consumer asks;
/// see [`AlarmKit::take_ring_ends`].
static RING_ENDS: std::sync::Mutex<Vec<(InstanceId, RingEnd)>> = std::sync::Mutex::new(Vec::new());

#[cfg(target_os = "android")]
const REMINDERS_CLASS: &str = "com/mobilesentinel/SentinelReminders";

/// See [`AlarmKit::on_ring_end`].
static RING_END_HOOK: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// Whether the OS delivering an alarm (the active marker, set at `marker_ms`)
/// is about the alarm's current state. A wake the app superseded since (it was
/// moved to a later time, dismissed, deleted) must not make it ring.
fn marker_is_current(state: &SessionState, marker_ms: Option<i64>) -> bool {
    match state {
        SessionState::Firing { .. } => true,
        SessionState::Scheduled { next_fire_unix_ms }
        | SessionState::Snoozed {
            next_fire_unix_ms, ..
        } => marker_ms.is_some_and(|m| *next_fire_unix_ms <= m + MARKER_SLOP_MS),
        SessionState::Idle | SessionState::Dismissed { .. } => false,
    }
}

/// When the alarm was due, if that time has passed while it was still waiting.
fn overdue_since(state: &SessionState, now_ms: i64) -> Option<i64> {
    match state {
        SessionState::Scheduled { next_fire_unix_ms }
        | SessionState::Snoozed {
            next_fire_unix_ms, ..
        } if *next_fire_unix_ms <= now_ms => Some(*next_fire_unix_ms),
        _ => None,
    }
}

fn has_no_next_fire(e: &AlarmKitError) -> bool {
    matches!(
        e,
        AlarmKitError::Recipe(RecipeError::Recurrence(
            crate::recipes::recurrence::RecurrenceError::NoFireTime
        ))
    )
}

fn session_of(record: &ContextRecord) -> Option<AlarmKitSession> {
    let SessionState::Firing { fired_at_unix_ms } = record.state else {
        return None;
    };
    let RecipeContext::AlarmClass(ctx) = &record.context else {
        return None;
    };
    Some(AlarmKitSession {
        instance_id: record.id.clone(),
        fired_at_unix_ms,
        first_fired_unix_ms: ctx.first_fired_unix_ms.unwrap_or(fired_at_unix_ms),
        spec: AlarmSpec::from_context(ctx),
        snooze_count: ctx.snooze_count,
        challenges_solved: ctx.challenges_solved,
    })
}

/// Whether two specs differ in nothing but the time zone.
fn same_but_for_zone(a: &AlarmSpec, b: &AlarmSpec) -> bool {
    let mut b = b.clone();
    b.time_zone.clone_from(&a.time_zone);
    *a == b
}

/// The zone to schedule in. A zone the device reports but the time zone database
/// doesn't know (a custom "GMT+01:00") becomes its fixed offset. When the device
/// can't be asked at all, the last known zone is used, because a wrong zone moves
/// alarms by hours.
fn choose_zone(device: Option<&str>, last_known: Option<&str>, offset_secs: i32) -> String {
    let known = |z: &&str| z.parse::<chrono_tz::Tz>().is_ok();
    if let Some(zone) = device.filter(known) {
        return zone.to_owned();
    }
    if device.is_none() {
        if let Some(zone) = last_known.filter(known) {
            return zone.to_owned();
        }
    }
    // Etc/GMT zones have the sign reversed: Etc/GMT-1 is one hour ahead of UTC.
    match offset_secs / 3600 {
        0 => "UTC".to_owned(),
        hours if hours > 0 => format!("Etc/GMT-{hours}"),
        hours => format!("Etc/GMT+{}", -hours),
    }
}

#[derive(Debug, Error)]
pub enum AlarmKitError {
    #[error("alarm not found: {0}")]
    NotFound(InstanceId),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("recipe error: {0}")]
    Recipe(#[from] RecipeError),
}

impl AlarmKit {
    /// One-time install. Wires the global AlarmClass runtime, registers
    /// the AlarmClass Recipe (idempotent), and returns a handle.
    ///
    /// Calling `install` more than once with different stores is undefined
    /// behaviour by design — the AlarmClass runtime is a singleton.
    ///
    /// Also persists a runtime manifest to disk so the `:sentinel` child
    /// process can self-bootstrap when an alarm broadcast wakes it up.
    pub fn install(config: AlarmKitConfig) -> Self {
        let runtime = alarm_class_runtime();
        runtime.set_store(config.store.clone());
        if let Some(sink) = config.sink {
            runtime.set_sink(sink);
        }
        if let Some(resolver) = config.sound_resolver {
            runtime.set_sound_resolver(resolver);
        }
        runtime.configure(config.alarm_class_config.clone());
        // Idempotent — duplicate registration is silently ignored.
        let _ = register_recipe(AlarmClass::new());

        let kit = Self {
            store: config.store,
        };

        // Register default job heads-up callback. When the :sentinel
        // job guardian sends a heads-up (MAIN is alive, active job
        // exists), this default handler re-engages or fires the alarm.
        // Consumers can override by calling `on_job_heads_up` again
        // after `install()` if they need custom behavior (e.g. flash
        // torch, custom vibration pattern).
        #[cfg(target_os = "android")]
        {
            crate::platform::callbacks::on_job_heads_up(|job_id| {
                let rt = alarm_class_runtime();
                if let Some(store) = rt.store() {
                    let kit_handle = AlarmKit { store };
                    kit_handle.handle_job_heads_up(&job_id);
                }
            });
        }

        kit
    }

    /// Construct a handle for an already-installed runtime. Use this
    /// from background processes (`:sentinel`) where `install` was already
    /// called by `Application.onCreate`.
    pub fn handle(store: Arc<ContextStore>) -> Self {
        Self { store }
    }

    /// Borrow the underlying ContextStore. Useful for advanced cases
    /// (custom queries, migrations).
    pub fn store(&self) -> &Arc<ContextStore> {
        &self.store
    }

    // -----------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------

    /// Create a new alarm. Returns the freshly-allocated [`InstanceId`].
    /// Persists the Context and dispatches `Trigger::Schedule` so the
    /// next-fire time is computed and an exact alarm is armed.
    pub fn create(&self, spec: AlarmSpec) -> Result<InstanceId, AlarmKitError> {
        let id = InstanceId::new(Uuid::new_v4().to_string());
        self.create_with_id(id.clone(), spec)?;
        Ok(id)
    }

    /// Variant of [`Self::create`] that lets the caller choose the
    /// instance id (useful for migrations from legacy stores that have
    /// existing UUIDs).
    pub fn create_with_id(&self, id: InstanceId, spec: AlarmSpec) -> Result<(), AlarmKitError> {
        let context = RecipeContext::AlarmClass(spec.into_context());
        let record = ContextRecord::new(id.clone(), context);
        self.store.write(record)?;
        // Schedule arms the next exact alarm via the sink. Dispatch
        // re-loads the just-written Context (LoadContext-on-every-trigger).
        dispatch_trigger(&self.store, Trigger::Schedule, &id)?;
        Ok(())
    }

    /// Update an alarm's spec and re-arm it with the new schedule. An alarm that
    /// did not change and is already armed stays exactly as it is, and a snooze
    /// in progress also survives a time zone change, because it is a moment in
    /// time, not a time of day.
    pub fn update(&self, id: &InstanceId, spec: AlarmSpec) -> Result<(), AlarmKitError> {
        let transition = crate::recipes::dispatch::lock_transitions();
        let mut record = self.store.load(id)?;
        let mut keep_counts = (0, false, None);
        if let RecipeContext::AlarmClass(old) = &record.context {
            keep_counts = (
                old.snooze_count,
                old.challenges_solved,
                old.first_fired_unix_ms,
            );
            let old_spec = AlarmSpec::from_context(old);
            let armed = match record.state {
                SessionState::Scheduled { next_fire_unix_ms } => {
                    next_fire_unix_ms > Utc::now().timestamp_millis()
                }
                SessionState::Snoozed { .. } | SessionState::Firing { .. } => true,
                _ => false,
            };
            if old_spec == spec && armed {
                return Ok(());
            }
            if matches!(record.state, SessionState::Snoozed { .. })
                && same_but_for_zone(&old_spec, &spec)
            {
                let mut context = spec.into_context();
                context.snooze_count = keep_counts.0;
                context.first_fired_unix_ms = keep_counts.2;
                record.context = RecipeContext::AlarmClass(context);
                self.store.write(record)?;
                return Ok(());
            }
        }
        let mut context = spec.into_context();
        // A firing session is not disturbed: the consumer must dismiss first, and
        // the next dismiss or snooze re-arms naturally.
        if matches!(record.state, SessionState::Firing { .. }) {
            (
                context.snooze_count,
                context.challenges_solved,
                context.first_fired_unix_ms,
            ) = keep_counts;
            record.context = RecipeContext::AlarmClass(context);
            self.store.write(record)?;
            return Ok(());
        }
        record.context = RecipeContext::AlarmClass(context);
        // Reset state to Idle so Edit recomputes cleanly.
        record.state = SessionState::Idle;
        self.store.write(record)?;
        drop(transition);
        dispatch_trigger(&self.store, Trigger::Edit, id)?;
        Ok(())
    }

    /// Delete an alarm. Cancels its pending exact alarm and removes the
    /// stored Context. Idempotent.
    pub fn delete(&self, id: &InstanceId) -> Result<(), AlarmKitError> {
        let ringing = matches!(
            self.store.load(id).map(|r| r.state),
            Ok(SessionState::Firing { .. })
        );
        // Only this alarm's ring: another alarm may be ringing right now.
        if ringing {
            alarm_class::tear_down_ring(&self.store, id);
        }
        if let Some(sink) = alarm_class_runtime().sink() {
            sink.cancel_exact_alarm(&id.0);
        }
        let _ = crate::jobs::deactivate_job(&id.0);
        self.store.delete(id)?;
        Ok(())
    }

    /// Snooze the currently-firing alarm. Idempotent.
    ///
    /// Transitions to Snoozed and arms the next wake at the snooze interval. The
    /// sentinel job stops being active, so the guardian stops resurrecting MAIN
    /// between the snooze and the re-fire; the re-fire's alarm broadcast
    /// activates the same job again, bringing MAIN back when it should ring.
    pub fn snooze(&self, id: &InstanceId) -> Result<(), AlarmKitError> {
        dispatch_trigger(&self.store, Trigger::Snooze, id)?;
        Ok(())
    }

    /// Dismiss an alarm. Transitions to Dismissed, arms the next occurrence for
    /// recurring alarms, and the sentinel job stops being active.
    ///
    /// If the alarm has challenges and is currently firing, this returns
    /// `Err(AlarmKitError::Recipe(RecipeError::ChallengesUnsolved(_)))` until
    /// [`Self::mark_solved`] has been called. The job stays active in that case.
    pub fn dismiss(&self, id: &InstanceId) -> Result<(), AlarmKitError> {
        dispatch_trigger(&self.store, Trigger::Dismiss, id)?;
        Ok(())
    }

    /// Notify AlarmClass that the user has solved the challenge gate
    /// for the currently-firing instance. The next `dismiss(id)` call
    /// will succeed.
    pub fn mark_solved(&self, id: &InstanceId) -> Result<(), AlarmKitError> {
        dispatch_trigger(&self.store, Trigger::Solve, id)?;
        Ok(())
    }

    /// Re-engage firing surfaces for an instance that's already in Firing
    /// state. Called when MAIN restarts mid-fire (killed by user or OS).
    /// Does NOT change state — just restarts audio, FGS, kiosk. No-op if
    /// the instance is not in Firing state.
    ///
    /// Routes through `Trigger::Resume` so the re-engage path is identical
    /// to [`Self::resume`] and the firing surface is built in exactly one
    /// place (the AlarmClass recipe). `reengage` and `resume` are aliases;
    /// both mean "bring the firing surface back without changing state."
    pub fn reengage(&self, id: &InstanceId) -> Result<(), AlarmKitError> {
        dispatch_trigger(&self.store, Trigger::Resume, id)?;
        Ok(())
    }

    /// Handle a job heads-up from the sentinel job guardian. Reads the
    /// job file, extracts the `instance_id` from the payload, and
    /// re-engages firing surfaces. This is how MAIN learns "an alarm
    /// needs to fire" when the app is already alive.
    pub fn handle_job_heads_up(&self, job_id: &str) {
        let job = match crate::jobs::get_job(job_id) {
            Ok(Some(j)) => j,
            _ => {
                log::warn!("[AlarmKit] handle_job_heads_up: job '{}' not found", job_id);
                return;
            }
        };
        let instance_id_str = match job.payload.get("instance_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => {
                log::warn!(
                    "[AlarmKit] handle_job_heads_up: job '{}' has no instance_id in payload",
                    job_id
                );
                return;
            }
        };
        self.engage_or_fire(&InstanceId::new(instance_id_str.to_owned()));
    }

    /// Shared helper for an instance whose job is active: re-engage firing
    /// surfaces if it already rings, otherwise fire it. A marker that has nothing
    /// to ring (the alarm was deleted, or moved on since) is dropped: it would
    /// keep the guardian relaunching the app for no alarm.
    fn engage_or_fire(&self, id: &InstanceId) {
        if let Some(end) = crate::jobs::ring_end(&id.0) {
            if self.end_session(id) {
                RING_ENDS
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((id.clone(), end));
                if let Some(hook) = RING_END_HOOK.get() {
                    hook();
                }
                return;
            }
        }
        let record = match self.store.load(id) {
            Ok(r) => r,
            Err(StoreError::NotFound(_)) => {
                let _ = crate::jobs::deactivate_job(&id.0);
                return;
            }
            Err(e) => {
                log::warn!("[AlarmKit] engage_or_fire: load '{}' failed: {:?}", id.0, e);
                return;
            }
        };
        if !marker_is_current(&record.state, crate::jobs::active_since_ms(&id.0)) {
            log::info!("[AlarmKit] dropping stale active marker of {}", id.0);
            let _ = crate::jobs::deactivate_job(&id.0);
            return;
        }
        if matches!(record.state, SessionState::Firing { .. }) {
            let _ = self.reengage(id);
        } else {
            let _ = dispatch_trigger(&self.store, Trigger::Fire, id);
        }
    }

    /// The user is doing something about a ringing alarm: its limit counts from
    /// now. Touches on the alarm screen count by themselves while the kiosk lock is
    /// on, so this is for what the platform cannot see, like shaking or walking.
    /// Cheap and throttled, so call it as often as it happens.
    pub fn note_activity(&self) {
        static LAST: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
        let now_ms = Utc::now().timestamp_millis();
        if now_ms - LAST.load(std::sync::atomic::Ordering::Relaxed) < ACTIVITY_THROTTLE_MS {
            return;
        }
        LAST.store(now_ms, std::sync::atomic::Ordering::Relaxed);
        let file = crate::utilities::app_files_dir().join(ACTIVITY_FILE);
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, "");
    }

    /// The record of how fast rings were, one `<unix ms> <event> <alarm>` per line,
    /// for a user to send in. Events: `delivered`, `service`, `sound`, `no_sound`,
    /// `ui`. Empty when nothing has rung yet.
    pub fn timing_log(&self) -> String {
        std::fs::read_to_string(crate::utilities::app_files_dir().join(TIMING_FILE))
            .unwrap_or_default()
    }

    /// Add to the record of [`Self::timing_log`] that the alarm's screen is up.
    pub fn note_ui_up(&self, id: &InstanceId) {
        use std::io::Write;
        let file = crate::utilities::app_files_dir().join(TIMING_FILE);
        if let Ok(mut log) = std::fs::OpenOptions::new().append(true).open(file) {
            let _ = writeln!(log, "{} ui {}", Utc::now().timestamp_millis(), id.0);
        }
    }

    /// Have `hook` called right after the platform ended a ring while the app runs,
    /// for it to [`Self::take_ring_ends`]. The app may have no UI to look for them, then
    /// or ever: it can run without a screen, and die again. Set once, on any thread.
    pub fn on_ring_end(&self, hook: fn()) {
        let _ = RING_END_HOOK.set(hook);
    }

    /// Show a quiet notification at `at_ms`, replacing an earlier one for this alarm. It
    /// has nothing to do with the ring, and may come a few minutes off on a dozing phone.
    /// It goes away after `lasts_ms`. `channel_name` is what the user sees in the phone's
    /// settings. No-op off Android.
    pub fn set_reminder(
        &self,
        id: &InstanceId,
        at_ms: i64,
        title: &str,
        body: &str,
        channel_name: &str,
        lasts_ms: i64,
    ) {
        #[cfg(target_os = "android")]
        {
            use crate::platform::android::jni::{jni_str, with_jni_class};
            use ::jni::objects::JValue;
            with_jni_class(REMINDERS_CLASS, false, |env, class| {
                let id = jni_str(env, &id.0)?;
                let title = jni_str(env, title)?;
                let body = jni_str(env, body)?;
                let channel = jni_str(env, channel_name)?;
                env.call_static_method(
                    class,
                    "set",
                    "(Ljava/lang/String;JLjava/lang/String;Ljava/lang/String;Ljava/lang/String;J)Z",
                    &[
                        JValue::Object(&id),
                        JValue::Long(at_ms),
                        JValue::Object(&title),
                        JValue::Object(&body),
                        JValue::Object(&channel),
                        JValue::Long(lasts_ms),
                    ],
                )
                .ok()
                .and_then(|v| v.z().ok())
            });
        }
        #[cfg(not(target_os = "android"))]
        let _ = (id, at_ms, title, body, channel_name, lasts_ms);
    }

    /// Take away what [`Self::set_reminder`] set for this alarm.
    pub fn clear_reminder(&self, id: &InstanceId) {
        #[cfg(target_os = "android")]
        {
            use crate::platform::android::jni::{jni_str, with_jni_class};
            use ::jni::objects::JValue;
            with_jni_class(REMINDERS_CLASS, false, |env, class| {
                let id = jni_str(env, &id.0)?;
                env.call_static_method(
                    class,
                    "clear",
                    "(Ljava/lang/String;)Z",
                    &[JValue::Object(&id)],
                )
                .ok()
                .and_then(|v| v.z().ok())
            });
        }
        #[cfg(not(target_os = "android"))]
        let _ = id;
    }

    /// Rings the platform ended while the app was running, since the last call. The
    /// consumer may want to tell the user and switch off a one-time alarm.
    pub fn take_ring_ends(&self) -> Vec<(InstanceId, RingEnd)> {
        std::mem::take(
            &mut RING_ENDS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Pause audio (e.g., during a telephony interruption).
    pub fn pause(&self, id: &InstanceId) -> Result<(), AlarmKitError> {
        dispatch_trigger(&self.store, Trigger::Pause, id)?;
        Ok(())
    }

    /// Resume a paused firing session: re-engage all firing surfaces
    /// (audio, FGS, kiosk) without changing state. This brings the sound
    /// back after a challenge timeout or any temporary pause. Alias for
    /// [`Self::reengage`] — both route through `Trigger::Resume`.
    pub fn resume(&self, id: &InstanceId) -> Result<(), AlarmKitError> {
        self.reengage(id)
    }

    // -----------------------------------------------------------------
    // Inspection
    // -----------------------------------------------------------------

    /// List every persisted alarm with its current spec. Sort order is
    /// not stable.
    pub fn list(&self) -> Result<Vec<(InstanceId, AlarmSpec)>, AlarmKitError> {
        let records = self.store.enumerate_all()?;
        let mut out = Vec::with_capacity(records.len());
        for record in records {
            if let RecipeContext::AlarmClass(ctx) = &record.context {
                out.push((record.id.clone(), AlarmSpec::from_context(ctx)));
            }
        }
        Ok(out)
    }

    /// Read a single alarm spec.
    pub fn get(&self, id: &InstanceId) -> Result<AlarmSpec, AlarmKitError> {
        let record = self.store.load(id)?;
        match &record.context {
            RecipeContext::AlarmClass(ctx) => Ok(AlarmSpec::from_context(ctx)),
            _ => Err(AlarmKitError::NotFound(id.clone())),
        }
    }

    /// Next fire instant (UTC unix ms) for a Scheduled or Snoozed alarm.
    /// Returns `None` for Idle / Firing / Dismissed states.
    pub fn next_fire(&self, id: &InstanceId) -> Result<Option<i64>, AlarmKitError> {
        let record = self.store.load(id)?;
        Ok(match record.state {
            SessionState::Scheduled { next_fire_unix_ms }
            | SessionState::Snoozed {
                next_fire_unix_ms, ..
            } => Some(next_fire_unix_ms),
            _ => None,
        })
    }

    /// Snapshot of the alarm the user should deal with now: the one that has
    /// been firing longest, so the answer does not change while it rings.
    pub fn current_session(&self) -> Result<Option<AlarmKitSession>, AlarmKitError> {
        let mut records = self.store.enumerate_all()?;
        records.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        let mut current: Option<AlarmKitSession> = None;
        for session in records.iter().filter_map(session_of) {
            if current
                .as_ref()
                .map_or(true, |c| session.fired_at_unix_ms < c.fired_at_unix_ms)
            {
                current = Some(session);
            }
        }
        Ok(current)
    }

    /// Snapshot of this alarm if it is firing.
    pub fn session(&self, id: &InstanceId) -> Option<AlarmKitSession> {
        session_of(&self.store.load(id).ok()?)
    }

    /// Permissions every consumer needs to declare in its manifest. Read
    /// from the registered AlarmClass Recipe so this stays in sync with
    /// the Recipe's declarations.
    pub fn required_permissions(&self) -> Vec<&'static str> {
        recipe_registry()
            .get("alarm_class")
            .map(|r| {
                r.required_permissions()
                    .iter()
                    .map(|p| p.name)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------
    // Convenience
    // -----------------------------------------------------------------

    /// Single call that does everything needed on app startup:
    /// 1. End what cannot go on: a ring from before the current boot, and a
    ///    snooze that ended longer ago than the late-ring window.
    /// 2. Rearm all provided alarms. An alarm whose spec is unchanged keeps its
    ///    state, in particular a snooze in progress. One whose time has come
    ///    (the OS delivered it, or it is less than the late-ring window late)
    ///    rings; one later than that is reported as missed.
    /// 3. Process active jobs (engage the firing instances)
    /// 4. Reengage a firing session no active job covered
    pub fn on_startup(&self, alarms: &[(String, AlarmSpec)]) -> StartupReport {
        self.on_startup_at(
            alarms,
            crate::utilities::boot::boot_unix_ms(),
            Utc::now().timestamp_millis(),
        )
    }

    /// [`Self::on_startup`] for when the app was started without its UI (a boot,
    /// an update, the clock changing, an alarm going off).
    pub fn on_wake(&self, alarms: &[(String, AlarmSpec)]) -> StartupReport {
        self.sync_at(
            alarms,
            crate::utilities::boot::boot_unix_ms(),
            Utc::now().timestamp_millis(),
            LateRing::ByOsAlarm,
        )
    }

    /// [`Self::on_startup`] with the boot time and the clock injected.
    fn on_startup_at(
        &self,
        alarms: &[(String, AlarmSpec)],
        boot_unix_ms: Option<i64>,
        now_ms: i64,
    ) -> StartupReport {
        self.sync_at(alarms, boot_unix_ms, now_ms, LateRing::Now)
    }

    fn sync_at(
        &self,
        alarms: &[(String, AlarmSpec)],
        boot_unix_ms: Option<i64>,
        now_ms: i64,
        late_ring: LateRing,
    ) -> StartupReport {
        let mut report = StartupReport::default();
        let late_window = alarm_class_runtime().config().late_ring_window_ms;

        // 1. A reboot kills the process, the sound and the lock screen takeover.
        //    A ring from before the boot therefore counts as dismissed (it must
        //    not ring again when the app is merely opened).
        if let Ok(records) = self.store.enumerate_all() {
            for record in records {
                let ring_from_before_boot = matches!(
                    record.state,
                    SessionState::Firing { fired_at_unix_ms }
                        if boot_unix_ms.is_some_and(|boot| fired_at_unix_ms < boot)
                );
                let snooze_too_late = match record.state {
                    SessionState::Snoozed {
                        next_fire_unix_ms, ..
                    } if now_ms - next_fire_unix_ms > late_window => Some(next_fire_unix_ms),
                    _ => None,
                };
                // Nothing ran between that ring and this start, and the alarm went
                // off again in the meantime. The old ring is over, but this one
                // must go on: it is ringing right now.
                let rang_again_since_boot = crate::jobs::active_since_ms(&record.id.0)
                    .is_some_and(|marker| boot_unix_ms.is_some_and(|boot| marker >= boot));
                if ring_from_before_boot && rang_again_since_boot {
                    let mut fresh = record.clone();
                    fresh.state = SessionState::Firing {
                        fired_at_unix_ms: now_ms,
                    };
                    if let RecipeContext::AlarmClass(ctx) = &mut fresh.context {
                        ctx.snooze_count = 0;
                        ctx.challenges_solved = false;
                        ctx.first_fired_unix_ms = Some(now_ms);
                    }
                    if self.store.write(fresh).is_ok() {
                        report.ended_by_reboot.push(record.id.clone());
                    }
                } else if ring_from_before_boot {
                    if self.end_session(&record.id) {
                        report.ended_by_reboot.push(record.id.clone());
                    }
                } else if let Some(end) = crate::jobs::ring_end(&record.id.0) {
                    if self.end_session(&record.id) {
                        report.ring_ends.push((record.id.clone(), end));
                    }
                } else if let Some(due_unix_ms) = snooze_too_late {
                    if self.end_session(&record.id) {
                        report.missed.push(MissedAlarm {
                            id: record.id.clone(),
                            due_unix_ms,
                        });
                    }
                }
            }
        }

        // 2. Rearm the provided alarms.
        let mut due = Vec::new();
        for (id_str, spec) in alarms {
            let id = InstanceId::new(id_str.clone());
            match self.rearm(&id, spec, boot_unix_ms, now_ms, late_window) {
                Ok(Rearmed::Armed) => {}
                Ok(Rearmed::Due) => due.push(id),
                Ok(Rearmed::Missed {
                    due_unix_ms,
                    finished,
                }) => {
                    if finished {
                        report.finished.push(id.clone());
                    }
                    report.missed.push(MissedAlarm { id, due_unix_ms });
                }
                Ok(Rearmed::Finished) => report.finished.push(id),
                Err(e) => log::warn!("[AlarmKit] on_startup rearm failed for {id_str}: {e:?}"),
            }
        }

        // 3. Process active jobs — for any active job with an instance_id
        //    payload, engage_or_fire. Each instance is engaged once: every
        //    engage restarts the audio and launches the activity again.
        let mut engaged = std::collections::HashSet::new();
        if let Ok(jobs) = crate::jobs::get_active_jobs() {
            for job in &jobs {
                if let Some(instance_id_str) =
                    job.payload.get("instance_id").and_then(|v| v.as_str())
                {
                    if engaged.insert(instance_id_str.to_owned()) {
                        self.engage_or_fire(&InstanceId::new(instance_id_str.to_owned()));
                    }
                }
            }
        }

        // Late alarms no job covered: the OS never told us, but it is time.
        for id in due {
            if !engaged.insert(id.0.clone()) {
                continue;
            }
            if late_ring == LateRing::Now {
                let _ = dispatch_trigger(&self.store, Trigger::Fire, &id);
            } else if let Ok(record) = self.store.load(&id) {
                if let RecipeContext::AlarmClass(ctx) = &record.context {
                    alarm_class::arm_exact_alarm(&id, ctx, now_ms + LATE_RING_DELAY_MS);
                }
            }
        }

        // 4. Reengage a firing session that nothing above covered.
        if let Ok(Some(session)) = self.current_session() {
            if !engaged.contains(&session.instance_id.0) {
                let _ = self.reengage(&session.instance_id);
            }
        }
        report
    }

    /// Dismiss a session that cannot go on, bypassing the challenge gate (the
    /// user is not at the phone solving anything). Returns whether the instance
    /// was dismissed.
    fn end_session(&self, id: &InstanceId) -> bool {
        let firing = matches!(
            self.store.load(id).map(|r| r.state),
            Ok(SessionState::Firing { .. })
        );
        if firing {
            let _ = dispatch_trigger(&self.store, Trigger::Solve, id);
        }
        match self.dismiss(id) {
            Ok(()) => true,
            Err(e) => {
                log::warn!("[AlarmKit] ending {} failed: {:?}", id.0, e);
                false
            }
        }
    }

    /// Make sure `id` is stored with `spec` and armed, without disturbing a
    /// firing session or a snooze in progress.
    fn rearm(
        &self,
        id: &InstanceId,
        spec: &AlarmSpec,
        boot_unix_ms: Option<i64>,
        now_ms: i64,
        late_window: i64,
    ) -> Result<Rearmed, AlarmKitError> {
        // Arm it, or report that a one-shot alarm has nothing left to ring.
        let finish = |result: Result<(), AlarmKitError>| match result {
            Ok(()) => Ok(Rearmed::Armed),
            Err(e) if has_no_next_fire(&e) => Ok(Rearmed::Finished),
            Err(e) => Err(e),
        };

        let record = match self.store.load(id) {
            Ok(r) => r,
            Err(StoreError::NotFound(_)) => {
                return finish(self.create_with_id(id.clone(), spec.clone()))
            }
            Err(e) => return Err(e.into()),
        };
        let RecipeContext::AlarmClass(ctx) = &record.context else {
            return finish(self.create_with_id(id.clone(), spec.clone()));
        };
        if matches!(record.state, SessionState::Firing { .. }) {
            return Ok(Rearmed::Armed);
        }
        let current = AlarmSpec::from_context(ctx);

        if let Some(due_unix_ms) = overdue_since(&record.state, now_ms) {
            // The OS delivered it during this boot: this start is that ring.
            let delivered = crate::jobs::active_since_ms(&id.0).is_some_and(|marker| {
                boot_unix_ms.map_or(true, |boot| marker >= boot)
                    && due_unix_ms <= marker + MARKER_SLOP_MS
            });
            if delivered || now_ms - due_unix_ms <= late_window {
                if current != *spec {
                    let mut context = spec.clone().into_context();
                    context.snooze_count = ctx.snooze_count;
                    context.first_fired_unix_ms = ctx.first_fired_unix_ms;
                    let mut record = record.clone();
                    record.context = RecipeContext::AlarmClass(context);
                    self.store.write(record)?;
                }
                return Ok(Rearmed::Due);
            }
            let _ = crate::jobs::deactivate_job(&id.0);
            return match self.update(id, spec.clone()) {
                Ok(()) => Ok(Rearmed::Missed {
                    due_unix_ms,
                    finished: false,
                }),
                Err(e) if has_no_next_fire(&e) => Ok(Rearmed::Missed {
                    due_unix_ms,
                    finished: true,
                }),
                Err(e) => Err(e),
            };
        }

        if current != *spec {
            return finish(self.update(id, spec.clone()));
        }
        match record.state {
            // The exact alarm was cleared by the reboot; the stored fire
            // time is still the right one.
            SessionState::Snoozed {
                next_fire_unix_ms, ..
            } => {
                alarm_class::arm_exact_alarm(id, ctx, next_fire_unix_ms);
                Ok(Rearmed::Armed)
            }
            // Scheduled / Idle / Dismissed: recompute and arm the next fire.
            _ => finish(dispatch_trigger(&self.store, Trigger::Schedule, id).map_err(Into::into)),
        }
    }

    /// Arm every waiting alarm again without moving it. An alarm carries a plan
    /// of how to ring, made when it was armed: call this after changing a runtime
    /// config it depends on (minimum volume, texts).
    pub fn refresh_ring_plans(&self) {
        let now_ms = Utc::now().timestamp_millis();
        let Ok(records) = self.store.enumerate_all() else {
            return;
        };
        for record in records {
            let RecipeContext::AlarmClass(ctx) = &record.context else {
                continue;
            };
            if let SessionState::Scheduled { next_fire_unix_ms }
            | SessionState::Snoozed {
                next_fire_unix_ms, ..
            } = record.state
            {
                if next_fire_unix_ms > now_ms {
                    alarm_class::arm_exact_alarm(&record.id, ctx, next_fire_unix_ms);
                }
            }
        }
    }

    /// Static helper that detects the device timezone. Always a zone the
    /// scheduler understands; see [`choose_zone`] for what happens otherwise.
    pub fn detect_timezone() -> String {
        let offset_secs = chrono::Local::now().offset().local_minus_utc();
        #[cfg(not(target_os = "android"))]
        {
            let device = std::env::var("TZ").unwrap_or_else(|_| "UTC".to_owned());
            choose_zone(Some(&device), None, offset_secs)
        }
        #[cfg(target_os = "android")]
        {
            let device = Some(crate::platform::android::firing_sink_android::get_time_zone_id())
                .filter(|z| !z.is_empty());
            let memo = crate::utilities::app_files_dir().join("sentinel/last_time_zone");
            let last_known = std::fs::read_to_string(&memo).ok();
            let zone = choose_zone(device.as_deref(), last_known.as_deref(), offset_secs);
            if last_known.as_deref() != Some(zone.as_str()) && device.is_some() {
                if let Some(dir) = memo.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(&memo, &zone);
            }
            zone
        }
    }

    /// Compute next fire time for a given alarm instance from ContextStore.
    /// Returns `None` if the instance is not in `Scheduled` state.
    pub fn next_fire_time(&self, id: &InstanceId) -> Option<chrono::DateTime<chrono::Utc>> {
        let record = self.store.load(id).ok()?;
        if let SessionState::Scheduled { next_fire_unix_ms } = record.state {
            return chrono::DateTime::from_timestamp_millis(next_fire_unix_ms);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::firing::MockFiringSink;
    use crate::recipes::context::ContextStore;
    use crate::recipes::test_lock::lock_and_clear;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct NoopResolver;
    impl SoundResolver for NoopResolver {
        fn resolve(&self, _id: &SoundIdSpec) -> String {
            "test://sound".into()
        }
    }

    const LATE_WINDOW: i64 = 15 * 60_000;

    fn install_kit(dir: &std::path::Path) -> (AlarmKit, Arc<MockFiringSink>) {
        let store = Arc::new(ContextStore::new(dir));
        let sink: Arc<MockFiringSink> = Arc::new(MockFiringSink::new());
        let kit = AlarmKit::install(AlarmKitConfig {
            store: store.clone(),
            sink: Some(sink.clone()),
            sound_resolver: Some(Arc::new(NoopResolver)),
            alarm_class_config: AlarmClassConfig {
                channel_id: "test_channel".into(),
                channel_name: "Test".into(),
                firing_body: "Tap".into(),
                importance: 5,
                audio_usage: "alarm".into(),
                audio_content_type: "sonification".into(),
                activity_fqcn: Some("dev.test.MainActivity".into()),
                kiosk_debounce_ms: 50,
                kiosk_block_home: true,
                kiosk_block_back: true,
                kiosk_block_recents: true,
                kiosk_block_volume: true,
                kiosk_hide_status_bar: false,
                kiosk_hide_nav_bar: false,
                firing_full_screen: true,
                vibration_pattern: vec![0, 400, 200, 400],
                min_volume_percent: 0,
                late_ring_window_ms: LATE_WINDOW,
                fallback_sound_id: Some(SoundIdSpec::Bundled("backup".into())),
                fallback_title: "Alarm".into(),
                backup_holds: 20,
                backup_hold_ms: 3_000,
                backup_text: "Hold".into(),
            },
        });
        (kit, sink)
    }

    fn weekday_spec() -> AlarmSpec {
        AlarmSpec {
            label: "Wake".into(),
            schedule: ScheduleSpec::Weekdays {
                days_mask: 0b0001_1111,
                hour: 7,
                minute: 0,
            },
            time_zone: "UTC".into(),
            sound_id: SoundIdSpec::Bundled("happy".into()),
            ..AlarmSpec::default()
        }
    }

    #[test]
    fn create_persists_spec_and_arms_exact_alarm() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        let id = kit.create(weekday_spec()).unwrap();
        let got = kit.get(&id).unwrap();
        assert_eq!(got.label, "Wake");
        assert_eq!(sink.schedule_count(), 1);
        let scheduled = sink.last_schedule().unwrap();
        assert_eq!(scheduled.alarm_id, id.0);
    }

    #[test]
    fn fire_request_carries_vibration_from_spec_and_pattern_from_config() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        // vibration_enabled defaults true in weekday_spec (via AlarmSpec::default).
        let id = kit.create(weekday_spec()).unwrap();
        // Force a fresh fire so a FireRequest is constructed.
        let mut record = kit.store().load(&id).unwrap();
        record.state = SessionState::Idle;
        kit.store().write(record).unwrap();
        dispatch_trigger(kit.store(), Trigger::Fire, &id).unwrap();

        let fire = sink.last_fire().expect("a FireRequest was issued");
        assert!(fire.vibrate, "vibration_enabled spec must set vibrate");
        // Pattern comes from the AlarmClassConfig the kit was installed with.
        assert_eq!(fire.vibration_pattern, vec![0, 400, 200, 400]);
    }

    #[test]
    fn fire_request_has_no_vibration_when_spec_disables_it() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        let mut spec = weekday_spec();
        spec.vibration_enabled = false;
        let id = kit.create(spec).unwrap();
        let mut record = kit.store().load(&id).unwrap();
        record.state = SessionState::Idle;
        kit.store().write(record).unwrap();
        dispatch_trigger(kit.store(), Trigger::Fire, &id).unwrap();

        let fire = sink.last_fire().expect("a FireRequest was issued");
        assert!(!fire.vibrate, "disabled spec must not vibrate");
    }

    #[test]
    fn per_alarm_vibration_pattern_overrides_config_default() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        // This alarm overrides the app-default pattern with its own.
        let mut spec = weekday_spec();
        spec.vibration_pattern = Some(vec![0, 100, 50, 100, 50, 800]);
        let id = kit.create(spec).unwrap();
        let mut record = kit.store().load(&id).unwrap();
        record.state = SessionState::Idle;
        kit.store().write(record).unwrap();
        dispatch_trigger(kit.store(), Trigger::Fire, &id).unwrap();

        let fire = sink.last_fire().expect("a FireRequest was issued");
        assert!(fire.vibrate);
        assert_eq!(
            fire.vibration_pattern,
            vec![0, 100, 50, 100, 50, 800],
            "per-alarm pattern must win over the AlarmClassConfig default"
        );

        // A sibling alarm with no override falls back to the config default.
        let sibling = kit.create(weekday_spec()).unwrap();
        let mut r2 = kit.store().load(&sibling).unwrap();
        r2.state = SessionState::Idle;
        kit.store().write(r2).unwrap();
        dispatch_trigger(kit.store(), Trigger::Fire, &sibling).unwrap();
        let fire2 = sink.last_fire().expect("a FireRequest was issued");
        assert_eq!(
            fire2.vibration_pattern,
            vec![0, 400, 200, 400],
            "an alarm with no override inherits the config default"
        );
    }

    #[test]
    fn update_replaces_spec_and_rearms() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        let id = kit.create(weekday_spec()).unwrap();
        let baseline = sink.schedule_count();

        let mut updated = weekday_spec();
        updated.label = "Snooze less".into();
        kit.update(&id, updated.clone()).unwrap();
        let got = kit.get(&id).unwrap();
        assert_eq!(got.label, "Snooze less");
        // Edit dispatches Schedule which calls schedule_exact_alarm again.
        assert!(sink.schedule_count() > baseline);
    }

    #[test]
    fn delete_cancels_alarm_and_removes_record() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        let id = kit.create(weekday_spec()).unwrap();
        kit.delete(&id).unwrap();
        assert!(matches!(
            kit.get(&id).unwrap_err(),
            AlarmKitError::Store(StoreError::NotFound(_))
        ));
        // Cancel was issued.
        assert!(sink.cancel_count() >= 1);
    }

    #[test]
    fn snooze_increments_count_and_rearms() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());

        let id = kit.create(weekday_spec()).unwrap();

        // Force into Firing state so snooze is applicable.
        let mut record = kit.store().load(&id).unwrap();
        record.state = SessionState::Firing {
            fired_at_unix_ms: chrono::Utc::now().timestamp_millis(),
        };
        kit.store().write(record).unwrap();

        kit.snooze(&id).unwrap();
        let session_state = kit.store().load(&id).unwrap().state;
        assert!(matches!(session_state, SessionState::Snoozed { .. }));
    }

    #[test]
    fn resume_reengages_firing_surface_when_firing() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        let id = kit.create(weekday_spec()).unwrap();
        let fires_before = sink.fire_count();

        // Force into Firing state, then resume — should re-issue start_firing
        // without changing state.
        let mut record = kit.store().load(&id).unwrap();
        record.state = SessionState::Firing {
            fired_at_unix_ms: chrono::Utc::now().timestamp_millis(),
        };
        kit.store().write(record).unwrap();

        kit.resume(&id).unwrap();
        assert_eq!(
            sink.fire_count(),
            fires_before + 1,
            "resume must re-engage the firing surface"
        );
        // State is unchanged by resume.
        assert!(matches!(
            kit.store().load(&id).unwrap().state,
            SessionState::Firing { .. }
        ));

        // reengage is an alias for resume — also re-engages.
        kit.reengage(&id).unwrap();
        assert_eq!(sink.fire_count(), fires_before + 2);
    }

    #[test]
    fn on_startup_engages_a_firing_instance_with_an_active_job_once() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        let id = kit.create(weekday_spec()).unwrap();
        let mut record = kit.store().load(&id).unwrap();
        record.state = SessionState::Firing {
            fired_at_unix_ms: chrono::Utc::now().timestamp_millis(),
        };
        kit.store().write(record).unwrap();
        // The process-global jobs dir is shared; the instance id is unique.
        crate::jobs::register_job(
            &id.0,
            serde_json::json!({ "instance_id": id.0 }),
            crate::jobs::JobConfig::default(),
        )
        .unwrap();
        crate::jobs::activate_job(&id.0).unwrap();

        let fires_before = sink.fire_count();
        kit.on_startup(&[]);
        let engaged = sink
            .fires
            .lock()
            .unwrap()
            .iter()
            .skip(fires_before)
            .filter(|f| f.instance_id == id.0)
            .count();
        let _ = crate::jobs::remove_job(&id.0);
        assert_eq!(
            engaged, 1,
            "one firing surface per instance, not one per source"
        );
    }

    fn force_state(kit: &AlarmKit, id: &InstanceId, state: SessionState) {
        let mut record = kit.store().load(id).unwrap();
        record.state = state;
        kit.store().write(record).unwrap();
    }

    #[test]
    fn on_startup_ends_a_ring_from_before_the_boot_and_keeps_a_later_one() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let now = 2_000_000_000_000_i64;
        let boot = now - 60_000;

        let before = kit.create(weekday_spec()).unwrap();
        let after = kit.create(weekday_spec()).unwrap();
        force_state(
            &kit,
            &before,
            SessionState::Firing {
                fired_at_unix_ms: boot - 5_000,
            },
        );
        force_state(
            &kit,
            &after,
            SessionState::Firing {
                fired_at_unix_ms: boot + 5_000,
            },
        );

        let report = kit.on_startup_at(&[], Some(boot), now);

        assert_eq!(report.ended_by_reboot, vec![before.clone()]);
        assert!(
            !matches!(
                kit.store().load(&before).unwrap().state,
                SessionState::Firing { .. }
            ),
            "a ring from before the boot is over"
        );
        assert!(sink.stops.lock().unwrap().contains(&before.0));
        assert!(
            matches!(
                kit.store().load(&after).unwrap().state,
                SessionState::Firing { .. }
            ),
            "a ring that started after the boot is left alone"
        );
    }

    #[test]
    fn on_startup_does_not_end_anything_when_the_boot_time_is_unknown() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());
        let id = kit.create(weekday_spec()).unwrap();
        force_state(
            &kit,
            &id,
            SessionState::Firing {
                fired_at_unix_ms: 1,
            },
        );

        let report = kit.on_startup_at(&[], None, 2_000_000_000_000);

        assert!(report.ended_by_reboot.is_empty());
        assert!(matches!(
            kit.store().load(&id).unwrap().state,
            SessionState::Firing { .. }
        ));
    }

    #[test]
    fn on_startup_keeps_a_snooze_in_progress_and_rearms_it() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let now = 2_000_000_000_000_i64;
        let spec = weekday_spec();
        let id = kit.create(spec.clone()).unwrap();
        let snoozed = SessionState::Snoozed {
            snooze_count: 1,
            next_fire_unix_ms: now + 300_000,
        };
        force_state(&kit, &id, snoozed.clone());

        let report = kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 60_000), now);

        assert!(report.ended_by_reboot.is_empty());
        assert_eq!(kit.store().load(&id).unwrap().state, snoozed);
        let last = sink.last_schedule().expect("snooze re-armed");
        assert_eq!(last.alarm_id, id.0);
        assert_eq!(last.target_unix_ms, now + 300_000);
    }

    /// The Snooze button ends the process, so this is how every snooze ends.
    #[test]
    fn a_snooze_that_runs_out_with_the_process_dead_rings() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let now = Utc::now().timestamp_millis();
        let spec = weekday_spec();
        let id = kit.create(spec.clone()).unwrap();
        force_state(
            &kit,
            &id,
            SessionState::Snoozed {
                snooze_count: 1,
                next_fire_unix_ms: now - 2_000,
            },
        );
        active_job(&id);
        let fires = sink.fire_count();

        // Up for a day, no reboot anywhere near.
        let report = kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 86_400_000), now);

        assert_eq!(sink.fire_count(), fires + 1, "it rings");
        assert_eq!(report, StartupReport::default());
        assert!(matches!(
            kit.store().load(&id).unwrap().state,
            SessionState::Firing { .. }
        ));
        let _ = crate::jobs::remove_job(&id.0);
    }

    #[test]
    fn a_snooze_that_ended_during_a_restart_rings_if_it_is_not_too_late() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let now = Utc::now().timestamp_millis();
        let spec = weekday_spec();
        let id = kit.create(spec.clone()).unwrap();
        force_state(
            &kit,
            &id,
            SessionState::Snoozed {
                snooze_count: 1,
                next_fire_unix_ms: now - 5 * 60_000,
            },
        );
        let fires = sink.fire_count();

        let report = kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 60_000), now);

        assert_eq!(sink.fire_count(), fires + 1);
        assert!(report.missed.is_empty());
    }

    #[test]
    fn a_snooze_that_ended_too_long_ago_is_reported_as_missed() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let now = Utc::now().timestamp_millis();
        let due = now - LATE_WINDOW - 60_000;
        let spec = weekday_spec();
        let id = kit.create(spec.clone()).unwrap();
        force_state(
            &kit,
            &id,
            SessionState::Snoozed {
                snooze_count: 1,
                next_fire_unix_ms: due,
            },
        );
        let fires = sink.fire_count();

        let report = kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 60_000), now);

        assert_eq!(sink.fire_count(), fires, "too late to ring");
        assert_eq!(
            report.missed,
            vec![MissedAlarm {
                id: id.clone(),
                due_unix_ms: due
            }]
        );
        assert!(report.ended_by_reboot.is_empty());
        assert!(matches!(
            kit.store().load(&id).unwrap().state,
            SessionState::Scheduled { .. }
        ));
    }

    fn active_job(id: &InstanceId) {
        crate::jobs::register_job(
            &id.0,
            serde_json::json!({ "instance_id": id.0 }),
            crate::jobs::JobConfig::default(),
        )
        .unwrap();
        crate::jobs::activate_job(&id.0).unwrap();
    }

    fn one_time_spec(date: chrono::NaiveDate) -> AlarmSpec {
        AlarmSpec {
            label: "Once".into(),
            schedule: ScheduleSpec::OneTime {
                date: date.format("%Y-%m-%d").to_string(),
                hour: 7,
                minute: 0,
            },
            time_zone: "UTC".into(),
            ..AlarmSpec::default()
        }
    }

    /// A process dead at ring time is the normal case.
    #[test]
    fn a_one_time_alarm_the_os_delivered_rings_and_is_not_finished() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let now = Utc::now().timestamp_millis();
        let spec = one_time_spec(Utc::now().date_naive() - chrono::Days::new(1));
        let id = InstanceId::new(Uuid::new_v4().to_string());
        let _ = kit.create_with_id(id.clone(), spec.clone());
        force_state(
            &kit,
            &id,
            SessionState::Scheduled {
                next_fire_unix_ms: now - 1_000,
            },
        );
        active_job(&id);
        let fires = sink.fire_count();

        let report = kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 86_400_000), now);

        assert_eq!(sink.fire_count(), fires + 1, "it rings");
        assert!(report.finished.is_empty(), "it is not over yet");
        assert!(matches!(
            kit.store().load(&id).unwrap().state,
            SessionState::Firing { .. }
        ));
        let _ = crate::jobs::remove_job(&id.0);
    }

    #[test]
    fn a_snoozed_one_time_alarm_keeps_its_snooze_at_startup() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let now = Utc::now().timestamp_millis();
        let spec = one_time_spec(Utc::now().date_naive() - chrono::Days::new(1));
        let id = InstanceId::new(Uuid::new_v4().to_string());
        let _ = kit.create_with_id(id.clone(), spec.clone());
        let snoozed = SessionState::Snoozed {
            snooze_count: 1,
            next_fire_unix_ms: now + 300_000,
        };
        force_state(&kit, &id, snoozed.clone());
        let fires = sink.fire_count();

        let report = kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 60_000), now);

        assert_eq!(kit.store().load(&id).unwrap().state, snoozed);
        assert_eq!(sink.fire_count(), fires);
        assert_eq!(report, StartupReport::default());
    }

    #[test]
    fn a_one_time_alarm_long_past_is_missed_and_finished() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let now = Utc::now().timestamp_millis();
        let due = now - 3 * 3_600_000;
        let spec = one_time_spec(Utc::now().date_naive() - chrono::Days::new(1));
        let id = InstanceId::new(Uuid::new_v4().to_string());
        let _ = kit.create_with_id(id.clone(), spec.clone());
        force_state(
            &kit,
            &id,
            SessionState::Scheduled {
                next_fire_unix_ms: due,
            },
        );
        let fires = sink.fire_count();

        let report = kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 60_000), now);

        assert_eq!(sink.fire_count(), fires);
        assert_eq!(report.finished, vec![id.clone()]);
        assert_eq!(
            report.missed,
            vec![MissedAlarm {
                id,
                due_unix_ms: due
            }]
        );
    }

    #[test]
    fn a_dismissed_one_time_alarm_is_finished_at_startup() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());
        let now = Utc::now().timestamp_millis();
        let spec = one_time_spec(Utc::now().date_naive() - chrono::Days::new(1));
        let id = InstanceId::new(Uuid::new_v4().to_string());
        let _ = kit.create_with_id(id.clone(), spec.clone());
        force_state(
            &kit,
            &id,
            SessionState::Dismissed {
                dismissed_at_unix_ms: now - 60_000,
            },
        );

        let report = kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 60_000), now);

        assert_eq!(report.finished, vec![id]);
        assert!(report.missed.is_empty());
    }

    #[test]
    fn a_repeating_alarm_the_os_never_delivered_rings_late_within_the_window() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let now = Utc::now().timestamp_millis();
        let spec = weekday_spec();
        let id = kit.create(spec.clone()).unwrap();
        force_state(
            &kit,
            &id,
            SessionState::Scheduled {
                next_fire_unix_ms: now - 10 * 60_000,
            },
        );
        let fires = sink.fire_count();

        kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 60_000), now);

        assert_eq!(sink.fire_count(), fires + 1);
    }

    #[test]
    fn a_repeating_alarm_long_past_is_missed_and_armed_for_next_time() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let now = Utc::now().timestamp_millis();
        let due = now - 3 * 3_600_000;
        let spec = weekday_spec();
        let id = kit.create(spec.clone()).unwrap();
        force_state(
            &kit,
            &id,
            SessionState::Scheduled {
                next_fire_unix_ms: due,
            },
        );
        let fires = sink.fire_count();

        let report = kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 60_000), now);

        assert_eq!(sink.fire_count(), fires);
        assert!(report.finished.is_empty());
        assert_eq!(report.missed.len(), 1);
        match kit.store().load(&id).unwrap().state {
            SessionState::Scheduled { next_fire_unix_ms } => assert!(next_fire_unix_ms > now),
            other => panic!("expected Scheduled, got {other:?}"),
        }
    }

    #[test]
    fn a_marker_from_before_the_boot_is_no_delivery() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let now = Utc::now().timestamp_millis();
        let spec = weekday_spec();
        let id = kit.create(spec.clone()).unwrap();
        force_state(
            &kit,
            &id,
            SessionState::Scheduled {
                next_fire_unix_ms: now - 3 * 3_600_000,
            },
        );
        active_job(&id);
        let fires = sink.fire_count();

        // The boot is later than the marker that was just written.
        let report = kit.on_startup_at(&[(id.0.clone(), spec)], Some(now + 60_000), now);

        assert_eq!(sink.fire_count(), fires);
        assert_eq!(report.missed.len(), 1);
        assert_eq!(crate::jobs::active_since_ms(&id.0), None);
        let _ = crate::jobs::remove_job(&id.0);
    }

    #[test]
    fn a_marker_for_a_wake_the_alarm_moved_on_from_does_not_ring() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let now = Utc::now().timestamp_millis();
        let spec = weekday_spec();
        let id = kit.create(spec.clone()).unwrap();
        force_state(
            &kit,
            &id,
            SessionState::Scheduled {
                next_fire_unix_ms: now + 3_600_000,
            },
        );
        active_job(&id);
        let fires = sink.fire_count();

        kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 60_000), now);

        assert_eq!(sink.fire_count(), fires);
        assert_eq!(crate::jobs::active_since_ms(&id.0), None, "marker dropped");
        let _ = crate::jobs::remove_job(&id.0);
    }

    #[test]
    fn a_marker_for_a_deleted_alarm_is_dropped() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let id = InstanceId::new(Uuid::new_v4().to_string());
        active_job(&id);
        let fires = sink.fire_count();

        kit.on_startup_at(&[], None, Utc::now().timestamp_millis());

        assert_eq!(sink.fire_count(), fires);
        assert_eq!(crate::jobs::active_since_ms(&id.0), None);
        let _ = crate::jobs::remove_job(&id.0);
    }

    #[test]
    fn a_time_zone_change_keeps_a_snooze() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());
        let id = kit.create(weekday_spec()).unwrap();
        let snoozed = SessionState::Snoozed {
            snooze_count: 2,
            next_fire_unix_ms: 2_000_000_000_000,
        };
        force_state(&kit, &id, snoozed.clone());

        let mut moved = weekday_spec();
        moved.time_zone = "Asia/Tokyo".into();
        kit.update(&id, moved).unwrap();

        let record = kit.store().load(&id).unwrap();
        assert_eq!(record.state, snoozed);
        match record.context {
            RecipeContext::AlarmClass(ctx) => assert_eq!(ctx.time_zone, "Asia/Tokyo"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn saving_an_unchanged_alarm_touches_nothing() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let id = kit.create(weekday_spec()).unwrap();
        let (arms, cancels) = (sink.schedule_count(), sink.cancel_count());

        kit.update(&id, weekday_spec()).unwrap();

        assert_eq!(sink.schedule_count(), arms);
        assert_eq!(sink.cancel_count(), cancels);
    }

    /// A kill between a cancel and an arm would lose the alarm.
    #[test]
    fn re_arming_never_cancels_first() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let id = kit.create(weekday_spec()).unwrap();
        force_state(
            &kit,
            &id,
            SessionState::Firing {
                fired_at_unix_ms: 1,
            },
        );
        kit.snooze(&id).unwrap();
        let mut changed = weekday_spec();
        changed.label = "Other".into();
        kit.update(&id, changed).unwrap();

        assert!(
            !sink.cancels.lock().unwrap().contains(&id.0),
            "the wake is replaced, not cancelled"
        );
    }

    #[test]
    fn deleting_an_alarm_that_is_not_ringing_leaves_the_ring_alone() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let id = kit.create(weekday_spec()).unwrap();

        kit.delete(&id).unwrap();

        assert!(sink.stops.lock().unwrap().is_empty());
    }

    #[test]
    fn firing_keeps_the_next_occurrence_armed() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let id = kit.create(weekday_spec()).unwrap();
        let arms = sink.schedule_count();

        dispatch_trigger(&kit.store, Trigger::Fire, &id).unwrap();

        assert_eq!(sink.schedule_count(), arms + 1);
        assert!(
            sink.last_schedule().unwrap().target_unix_ms > Utc::now().timestamp_millis(),
            "armed for the future"
        );
    }

    #[test]
    fn a_zone_the_device_reports_is_used_when_it_is_known() {
        assert_eq!(
            choose_zone(Some("Europe/Amsterdam"), Some("Asia/Tokyo"), 3600),
            "Europe/Amsterdam"
        );
    }

    #[test]
    fn a_device_that_cannot_be_asked_keeps_the_last_known_zone() {
        assert_eq!(
            choose_zone(None, Some("Europe/Amsterdam"), 3600),
            "Europe/Amsterdam"
        );
        assert_eq!(choose_zone(None, Some("Not/AZone"), 3600), "Etc/GMT-1");
        assert_eq!(choose_zone(None, None, 7200), "Etc/GMT-2");
    }

    #[test]
    fn a_zone_the_database_does_not_know_becomes_its_fixed_offset() {
        assert_eq!(
            choose_zone(Some("GMT+01:00"), Some("Asia/Tokyo"), 3600),
            "Etc/GMT-1"
        );
        assert_eq!(choose_zone(Some("Custom"), None, -5 * 3600), "Etc/GMT+5");
        assert_eq!(choose_zone(Some("Custom"), None, 0), "UTC");
        assert!("Etc/GMT-1".parse::<chrono_tz::Tz>().is_ok());
        assert!("Etc/GMT+5".parse::<chrono_tz::Tz>().is_ok());
    }

    #[test]
    fn update_with_an_unchanged_spec_keeps_a_snooze_but_a_changed_one_replaces_it() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());
        let id = kit.create(weekday_spec()).unwrap();
        let snoozed = SessionState::Snoozed {
            snooze_count: 2,
            next_fire_unix_ms: 2_000_000_000_000,
        };
        force_state(&kit, &id, snoozed.clone());

        kit.update(&id, weekday_spec()).unwrap();
        assert_eq!(kit.store().load(&id).unwrap().state, snoozed);

        let mut changed = weekday_spec();
        changed.label = "Other".into();
        kit.update(&id, changed).unwrap();
        assert!(matches!(
            kit.store().load(&id).unwrap().state,
            SessionState::Scheduled { .. }
        ));
    }

    #[test]
    fn on_startup_reschedules_an_alarm_whose_spec_changed() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());
        let now = 2_000_000_000_000_i64;
        let id = kit.create(weekday_spec()).unwrap();
        force_state(
            &kit,
            &id,
            SessionState::Snoozed {
                snooze_count: 1,
                next_fire_unix_ms: now + 300_000,
            },
        );
        let mut changed = weekday_spec();
        changed.label = "Renamed".into();

        kit.on_startup_at(&[(id.0.clone(), changed.clone())], Some(now - 60_000), now);

        assert_eq!(kit.get(&id).unwrap(), changed);
        assert!(matches!(
            kit.store().load(&id).unwrap().state,
            SessionState::Scheduled { .. }
        ));
    }

    fn plan_of(req: &crate::firing::ExactAlarmRequest) -> serde_json::Value {
        serde_json::from_str(req.metadata_json.as_deref().expect("a ring plan")).unwrap()
    }

    #[test]
    fn an_armed_alarm_carries_the_plan_to_ring_it_by() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let mut spec = weekday_spec();
        spec.fade_in_secs = 15;

        kit.create(spec).unwrap();

        // Read by the ring service in Kotlin, which runs without any Rust.
        let plan = plan_of(&sink.last_schedule().unwrap());
        assert_eq!(plan["title"], "Wake");
        assert_eq!(plan["sound_uri"], "test://sound");
        assert_eq!(plan["fade_in_ms"], 15_000);
        assert_eq!(plan["vibrate"], true);
        assert_eq!(plan["firing_full_screen"], true);
        assert_eq!(plan["activity_fqcn"], "dev.test.MainActivity");
    }

    #[test]
    fn every_way_of_arming_an_alarm_gives_it_a_plan() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let id = kit.create(weekday_spec()).unwrap();

        // The next occurrence, armed while it rings.
        dispatch_trigger(&kit.store, Trigger::Fire, &id).unwrap();
        plan_of(&sink.last_schedule().unwrap());
        // The snooze.
        kit.snooze(&id).unwrap();
        plan_of(&sink.last_schedule().unwrap());
        // The snooze again after a restart.
        let now = Utc::now().timestamp_millis();
        kit.on_startup_at(&[(id.0.clone(), weekday_spec())], Some(now - 60_000), now);
        plan_of(&sink.last_schedule().unwrap());
        // The next occurrence after a dismiss.
        force_state(
            &kit,
            &id,
            SessionState::Firing {
                fired_at_unix_ms: now,
            },
        );
        kit.dismiss(&id).unwrap();
        plan_of(&sink.last_schedule().unwrap());
    }

    #[test]
    fn a_broken_sound_has_somewhere_to_fall_back_to() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        kit.create(weekday_spec()).unwrap();

        // NoopResolver resolves the chosen sound and the fallback alike, so only
        // the system default is left after removing duplicates.
        let plan = plan_of(&sink.last_schedule().unwrap());
        assert_eq!(
            plan["fallback_sound_uris"],
            serde_json::json!(["content://settings/system/default-alarm"])
        );
    }

    #[test]
    fn an_alarm_without_a_label_still_has_a_title() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let mut spec = weekday_spec();
        spec.label = String::new();

        kit.create(spec).unwrap();

        assert_eq!(plan_of(&sink.last_schedule().unwrap())["title"], "Alarm");
    }

    #[test]
    fn refreshing_the_plans_changes_them_without_moving_anything() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let waiting = kit.create(weekday_spec()).unwrap();
        let snoozed = kit.create(weekday_spec()).unwrap();
        let ringing = kit.create(weekday_spec()).unwrap();
        dispatch_trigger(&kit.store, Trigger::Fire, &snoozed).unwrap();
        kit.snooze(&snoozed).unwrap();
        dispatch_trigger(&kit.store, Trigger::Fire, &ringing).unwrap();
        let next_waiting = kit.next_fire(&waiting).unwrap().unwrap();
        let next_snoozed = kit.next_fire(&snoozed).unwrap().unwrap();

        let mut config = alarm_class_runtime().config();
        config.min_volume_percent = 80;
        alarm_class_runtime().configure(config);
        let before = sink.schedule_count();
        kit.refresh_ring_plans();

        let schedules = sink.schedules.lock().unwrap();
        let refreshed = &schedules[before..];
        assert_eq!(refreshed.len(), 2, "the ringing alarm is not armed again");
        assert!(refreshed
            .iter()
            .all(|r| plan_of(r)["min_volume_percent"] == 80));
        let target_of = |id: &InstanceId| {
            refreshed
                .iter()
                .find(|r| r.alarm_id == id.0)
                .map(|r| r.target_unix_ms)
        };
        assert_eq!(target_of(&waiting), Some(next_waiting));
        assert_eq!(target_of(&snoozed), Some(next_snoozed));
    }

    #[test]
    fn the_screen_stays_locked_until_the_last_ringing_alarm_ends() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let a = kit.create(weekday_spec()).unwrap();
        let b = kit.create(weekday_spec()).unwrap();
        let releases = || sink.releases.load(std::sync::atomic::Ordering::SeqCst);
        dispatch_trigger(&kit.store, Trigger::Fire, &a).unwrap();
        dispatch_trigger(&kit.store, Trigger::Fire, &b).unwrap();

        kit.dismiss(&a).unwrap();
        assert_eq!(*sink.stops.lock().unwrap(), vec![a.0.clone()]);
        assert_eq!(releases(), 0, "b still rings");
        assert!(matches!(
            kit.store().load(&b).unwrap().state,
            SessionState::Firing { .. }
        ));

        kit.snooze(&b).unwrap();
        assert_eq!(releases(), 1);
    }

    #[test]
    fn deleting_one_of_two_ringing_alarms_keeps_the_lock() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let a = kit.create(weekday_spec()).unwrap();
        let b = kit.create(weekday_spec()).unwrap();
        dispatch_trigger(&kit.store, Trigger::Fire, &a).unwrap();
        dispatch_trigger(&kit.store, Trigger::Fire, &b).unwrap();

        kit.delete(&a).unwrap();

        assert_eq!(*sink.stops.lock().unwrap(), vec![a.0.clone()]);
        assert_eq!(sink.releases.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn the_sound_fades_in_when_the_alarm_first_fires_but_not_when_it_resumes() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let mut spec = weekday_spec();
        spec.fade_in_secs = 15;
        let id = kit.create(spec).unwrap();

        dispatch_trigger(&kit.store, Trigger::Fire, &id).unwrap();
        assert_eq!(sink.last_fire().unwrap().fade_in_ms, 15_000);

        kit.resume(&id).unwrap();
        assert_eq!(
            sink.last_fire().unwrap().fade_in_ms,
            0,
            "bringing the sound back after a challenge starts at full volume"
        );
    }

    #[test]
    fn resume_is_noop_when_not_firing() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        // Freshly created alarm is Scheduled, not Firing.
        let id = kit.create(weekday_spec()).unwrap();
        let fires_before = sink.fire_count();

        kit.resume(&id).unwrap();
        assert_eq!(
            sink.fire_count(),
            fires_before,
            "resume on a non-firing alarm must not engage the firing surface"
        );
    }

    #[test]
    fn pause_quiets_audio_without_changing_state() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        let id = kit.create(weekday_spec()).unwrap();
        let mut record = kit.store().load(&id).unwrap();
        record.state = SessionState::Firing {
            fired_at_unix_ms: chrono::Utc::now().timestamp_millis(),
        };
        kit.store().write(record).unwrap();

        kit.pause(&id).unwrap();
        assert_eq!(
            sink.pauses.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "pause must quiet the firing audio"
        );
        // Pause does not change state.
        assert!(matches!(
            kit.store().load(&id).unwrap().state,
            SessionState::Firing { .. }
        ));
    }

    #[test]
    fn list_returns_every_alarm() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());

        let _a = kit.create(weekday_spec()).unwrap();
        let _b = kit.create(weekday_spec()).unwrap();
        let listed = kit.list().unwrap();
        assert_eq!(listed.len(), 2);
    }

    #[test]
    fn next_fire_returns_none_for_idle_or_firing() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());

        let id = kit.create(weekday_spec()).unwrap();
        // Create dispatches Schedule, so state is Scheduled.
        assert!(kit.next_fire(&id).unwrap().is_some());

        // Force Firing — next_fire returns None.
        let mut record = kit.store().load(&id).unwrap();
        record.state = SessionState::Firing {
            fired_at_unix_ms: 1,
        };
        kit.store().write(record).unwrap();
        assert!(kit.next_fire(&id).unwrap().is_none());
    }

    #[test]
    fn current_session_returns_firing_alarm() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());

        let id = kit.create(weekday_spec()).unwrap();
        // Force Firing.
        let mut record = kit.store().load(&id).unwrap();
        record.state = SessionState::Firing {
            fired_at_unix_ms: 5_000,
        };
        kit.store().write(record).unwrap();

        let session = kit.current_session().unwrap().unwrap();
        assert_eq!(session.instance_id, id);
        assert_eq!(session.fired_at_unix_ms, 5_000);
        assert_eq!(session.spec.label, "Wake");
    }

    #[test]
    fn current_session_is_the_one_firing_longest_and_then_the_next() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());
        let newer = kit.create(weekday_spec()).unwrap();
        let older = kit.create(weekday_spec()).unwrap();
        force_state(
            &kit,
            &newer,
            SessionState::Firing {
                fired_at_unix_ms: 2_000,
            },
        );
        force_state(
            &kit,
            &older,
            SessionState::Firing {
                fired_at_unix_ms: 1_000,
            },
        );

        let current = || kit.current_session().unwrap().map(|s| s.instance_id);
        assert_eq!(current(), Some(older.clone()));
        kit.dismiss(&older).unwrap();
        assert_eq!(current(), Some(newer.clone()));
        kit.dismiss(&newer).unwrap();
        assert_eq!(current(), None);
    }

    #[test]
    fn a_ring_after_a_reboot_nobody_handled_is_not_taken_for_the_old_one() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let id = kit.create(weekday_spec()).unwrap();
        // Rang yesterday, the phone restarted, and no Rust ran since: the phone
        // holds back the boot signal. This morning the OS delivers the next one.
        let now = Utc::now().timestamp_millis();
        force_state(
            &kit,
            &id,
            SessionState::Firing {
                fired_at_unix_ms: now - 86_400_000,
            },
        );
        active_job(&id);
        let boot = crate::jobs::active_since_ms(&id.0).unwrap() - 1_000;

        let report = kit.on_startup_at(&[(id.0.clone(), weekday_spec())], Some(boot), now);

        assert_eq!(
            report.ended_by_reboot,
            vec![id.clone()],
            "the old ring is over"
        );
        assert!(
            matches!(
                kit.store().load(&id).unwrap().state,
                SessionState::Firing { fired_at_unix_ms } if fired_at_unix_ms == now
            ),
            "but this one rings"
        );
        assert!(
            crate::jobs::active_since_ms(&id.0).is_some(),
            "and without its marker the sound would stop"
        );
        assert_eq!(sink.last_fire().unwrap().instance_id, id.0);
        let _ = crate::jobs::remove_job(&id.0);
    }

    #[test]
    fn a_late_alarm_found_without_the_ui_rings_through_the_os() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let id = kit.create(weekday_spec()).unwrap();
        let now = Utc::now().timestamp_millis();
        let due = now - 5 * 60_000;
        force_state(
            &kit,
            &id,
            SessionState::Scheduled {
                next_fire_unix_ms: due,
            },
        );

        let report = kit.sync_at(
            &[(id.0.clone(), weekday_spec())],
            Some(now - 60_000),
            now,
            LateRing::ByOsAlarm,
        );

        assert!(report.missed.is_empty());
        assert_eq!(sink.fire_count(), 0, "not started from here");
        let armed = sink.last_schedule().unwrap();
        assert_eq!(armed.alarm_id, id.0);
        assert_eq!(armed.target_unix_ms, now + LATE_RING_DELAY_MS);
        plan_of(&armed);
        assert!(
            matches!(
                kit.store().load(&id).unwrap().state,
                SessionState::Scheduled { .. }
            ),
            "firing starts when the OS delivers it"
        );
    }

    #[test]
    fn the_late_window_is_part_of_the_plan() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        kit.create(weekday_spec()).unwrap();

        assert_eq!(
            plan_of(&sink.last_schedule().unwrap())["late_ring_window_ms"],
            LATE_WINDOW
        );
    }

    fn limit_reached(id: &InstanceId) {
        let dir = crate::jobs::jobs_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{}.limit", id.0)), "").unwrap();
    }

    #[test]
    fn the_ring_limit_is_part_of_the_plan() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());
        let mut spec = weekday_spec();

        kit.create(spec.clone()).unwrap();
        assert_eq!(
            plan_of(&sink.last_schedule().unwrap())["ring_limit_secs"],
            1800,
            "half an hour unless the alarm says otherwise"
        );

        spec.ring_limit_secs = None;
        kit.create(spec).unwrap();
        assert!(plan_of(&sink.last_schedule().unwrap())["ring_limit_secs"].is_null());
    }

    #[test]
    fn an_older_record_gets_the_default_limit() {
        let mut value = serde_json::to_value(weekday_spec().into_context()).unwrap();
        value.as_object_mut().unwrap().remove("ring_limit_secs");
        let ctx: AlarmClassContext = serde_json::from_value(value).unwrap();
        assert_eq!(ctx.ring_limit_secs, Some(DEFAULT_RING_LIMIT_SECS));
    }

    #[test]
    fn a_ring_that_reached_its_limit_ends_past_the_gate_with_the_app_running() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());
        let mut spec = weekday_spec();
        spec.challenges = vec![ChallengeSpec {
            challenge_type: "math".into(),
            difficulty: 0,
            config: serde_json::json!({}),
        }];
        let id = kit.create(spec).unwrap();
        active_job(&id);
        dispatch_trigger(&kit.store, Trigger::Fire, &id).unwrap();
        limit_reached(&id);
        static HOOK_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        kit.on_ring_end(|| {
            HOOK_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });

        kit.handle_job_heads_up(&id.0);

        assert!(matches!(
            kit.store().load(&id).unwrap().state,
            SessionState::Scheduled { .. }
        ));
        assert_eq!(crate::jobs::ring_end(&id.0), None);
        assert_eq!(crate::jobs::active_since_ms(&id.0), None);
        assert_eq!(kit.take_ring_ends(), vec![(id.clone(), RingEnd::Limit)]);
        assert!(kit.take_ring_ends().is_empty());
        assert!(HOOK_CALLS.load(std::sync::atomic::Ordering::SeqCst) >= 1);
        let _ = crate::jobs::remove_job(&id.0);
    }

    #[test]
    fn a_ring_that_reached_its_limit_with_the_app_dead_is_reported_at_startup() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());
        let id = kit.create(weekday_spec()).unwrap();
        // The app never came up: Rust still has it as waiting, for a time long gone.
        let now = Utc::now().timestamp_millis();
        force_state(
            &kit,
            &id,
            SessionState::Scheduled {
                next_fire_unix_ms: now - 30 * 60_000,
            },
        );
        limit_reached(&id);

        let report = kit.on_startup_at(
            &[(id.0.clone(), weekday_spec())],
            Some(now - 3_600_000),
            now,
        );

        assert_eq!(report.ring_ends, vec![(id.clone(), RingEnd::Limit)]);
        assert!(report.missed.is_empty(), "it did ring");
        assert!(matches!(
            kit.store().load(&id).unwrap().state,
            SessionState::Scheduled { next_fire_unix_ms } if next_fire_unix_ms > now
        ));
        assert_eq!(crate::jobs::ring_end(&id.0), None);
    }

    #[test]
    fn solving_the_backup_screen_ends_the_alarm_past_the_gate_at_the_next_start() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());
        let mut spec = weekday_spec();
        spec.challenges = vec![ChallengeSpec {
            challenge_type: "math".into(),
            difficulty: 0,
            config: serde_json::json!({}),
        }];
        let id = kit.create(spec.clone()).unwrap();
        active_job(&id);
        dispatch_trigger(&kit.store, Trigger::Fire, &id).unwrap();
        let jobs = crate::jobs::jobs_dir();
        std::fs::write(jobs.join(format!("{}.dismiss", id.0)), "").unwrap();
        let now = Utc::now().timestamp_millis();

        let report = kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 3_600_000), now);

        assert_eq!(report.ring_ends, vec![(id.clone(), RingEnd::BackupScreen)]);
        assert!(matches!(
            kit.store().load(&id).unwrap().state,
            SessionState::Scheduled { .. }
        ));
        assert_eq!(crate::jobs::ring_end(&id.0), None);
        let _ = crate::jobs::remove_job(&id.0);
    }

    #[test]
    fn the_backup_challenge_is_part_of_the_plan() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, sink) = install_kit(dir.path());

        kit.create(weekday_spec()).unwrap();

        let plan = plan_of(&sink.last_schedule().unwrap());
        assert_eq!(plan["backup_holds"], 20);
        assert_eq!(plan["backup_hold_ms"], 3_000);
        assert_eq!(plan["backup_text"], "Hold");
    }

    #[test]
    fn a_session_knows_when_it_first_rang_across_its_snoozes() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());
        let id = kit.create(weekday_spec()).unwrap();
        let first_of = |kit: &AlarmKit| kit.session(&id).map(|s| s.first_fired_unix_ms);

        dispatch_trigger(&kit.store, Trigger::Fire, &id).unwrap();
        let first = first_of(&kit).expect("firing");
        assert_eq!(kit.session(&id).unwrap().fired_at_unix_ms, first);

        kit.snooze(&id).unwrap();
        assert_eq!(kit.session(&id), None, "not firing while snoozed");
        std::thread::sleep(std::time::Duration::from_millis(5));
        dispatch_trigger(&kit.store, Trigger::Fire, &id).unwrap();
        assert_eq!(first_of(&kit), Some(first));
        assert!(kit.session(&id).unwrap().fired_at_unix_ms > first);
        assert_eq!(kit.session(&id).unwrap().snooze_count, 1);

        kit.dismiss(&id).unwrap();
        dispatch_trigger(&kit.store, Trigger::Fire, &id).unwrap();
        assert!(first_of(&kit).unwrap() > first, "a new session starts over");
    }

    #[test]
    fn current_session_is_none_when_nothing_firing() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());

        let _id = kit.create(weekday_spec()).unwrap();
        assert!(kit.current_session().unwrap().is_none());
    }

    #[test]
    fn required_permissions_includes_exact_alarm() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());
        let perms = kit.required_permissions();
        assert!(perms.iter().any(|p| p.contains("SCHEDULE_EXACT_ALARM")));
    }
}

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
use crate::recipes::alarm_class::{
    alarm_class_runtime, AlarmClass, AlarmClassConfig, SoundResolver,
};
use crate::recipes::context::schema::{
    AlarmClassContext, ChallengeSpec, ContextRecord, RecipeContext, ScheduleSpec, SessionState,
    SnoozePolicy, SoundIdSpec,
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
    pub fired_at_unix_ms: i64,
    pub spec: AlarmSpec,
    pub snooze_count: u32,
    pub challenges_solved: bool,
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

    /// Update an alarm's spec. Cancels any pending exact alarm and
    /// re-arms with the new schedule.
    pub fn update(&self, id: &InstanceId, spec: AlarmSpec) -> Result<(), AlarmKitError> {
        let mut record = self.store.load(id)?;
        record.context = RecipeContext::AlarmClass(spec.into_context());
        // Reset state to Idle so Schedule recomputes cleanly. We don't
        // disturb a Firing session — the consumer must dismiss first.
        let was_firing = matches!(record.state, SessionState::Firing { .. });
        if !was_firing {
            record.state = SessionState::Idle;
        }
        self.store.write(record)?;
        if was_firing {
            // The session is currently fire-active; do NOT dispatch
            // Edit — that would call schedule_next which (even with the
            // firing-guard) would persist a fresh Scheduled state if
            // the firing-guard ever drifts out of sync. Skip the
            // dispatch entirely; the next dismiss/snooze handler will
            // re-arm naturally.
            return Ok(());
        }
        dispatch_trigger(&self.store, Trigger::Edit, id)?;
        Ok(())
    }

    /// Delete an alarm. Cancels its pending exact alarm and removes the
    /// stored Context. Idempotent.
    pub fn delete(&self, id: &InstanceId) -> Result<(), AlarmKitError> {
        if let Some(sink) = alarm_class_runtime().sink() {
            sink.stop_firing(&id.0);
            sink.cancel_exact_alarm(&id.0);
        }
        self.store.delete(id)?;
        Ok(())
    }

    /// Snooze the currently-firing alarm. Idempotent.
    ///
    /// Transitions to Snoozed and re-arms the next exact alarm at the
    /// snooze interval. Also deactivates the sentinel job (sets it back to
    /// Pending) so the job guardian stops resurrecting MAIN while the
    /// device is between the snooze and the re-fire. The snooze re-fire's
    /// alarm broadcast flips the same job Pending → Active again, bringing
    /// MAIN back up exactly when the alarm should ring.
    pub fn snooze(&self, id: &InstanceId) -> Result<(), AlarmKitError> {
        dispatch_trigger(&self.store, Trigger::Snooze, id)?;
        // Deactivate (NOT complete) the job — the firing session is over
        // for now, but the snooze re-fire will re-activate this same job.
        let _ = crate::jobs::deactivate_job(&id.0);
        Ok(())
    }

    /// Dismiss an alarm. Transitions to Dismissed, re-arms the next
    /// occurrence for recurring alarms, and deactivates the sentinel job
    /// so the job guardian stops polling.
    ///
    /// The job is *deactivated* (reset to Pending), not removed: the job
    /// file must survive so a future fire — the next occurrence of a
    /// recurring alarm — can re-activate the same job. The file is only
    /// removed when the alarm itself is deleted (`delete`). A one-shot
    /// alarm simply never re-fires, leaving a harmless Pending job behind
    /// until deletion.
    pub fn dismiss(&self, id: &InstanceId) -> Result<(), AlarmKitError> {
        dispatch_trigger(&self.store, Trigger::Dismiss, id)?;
        // Deactivate the sentinel job — the firing session is done and
        // MAIN no longer needs to stay alive, but the job file must
        // persist so a recurring alarm's next fire can re-activate it.
        let _ = crate::jobs::deactivate_job(&id.0);
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

    /// Shared helper: if the instance is already in `Firing` state,
    /// just re-engage firing surfaces (audio, FGS, kiosk). Otherwise
    /// dispatch `Trigger::Fire` to transition state and engage. Used
    /// by both `handle_job_heads_up` and `on_startup`.
    fn engage_or_fire(&self, id: &InstanceId) {
        let record = match self.store.load(id) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[AlarmKit] engage_or_fire: load '{}' failed: {:?}", id.0, e);
                return;
            }
        };
        if matches!(record.state, SessionState::Firing { .. }) {
            let _ = self.reengage(id);
        } else {
            let _ = dispatch_trigger(&self.store, Trigger::Fire, id);
        }
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

    /// Snapshot of the currently-firing alarm if one exists.
    pub fn current_session(&self) -> Result<Option<AlarmKitSession>, AlarmKitError> {
        for record in self.store.enumerate_all()? {
            if let SessionState::Firing { fired_at_unix_ms } = record.state {
                if let RecipeContext::AlarmClass(ctx) = &record.context {
                    return Ok(Some(AlarmKitSession {
                        instance_id: record.id.clone(),
                        fired_at_unix_ms,
                        spec: AlarmSpec::from_context(ctx),
                        snooze_count: ctx.snooze_count,
                        challenges_solved: ctx.challenges_solved,
                    }));
                }
            }
        }
        Ok(None)
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
    /// 1. Rearm all provided alarms (skip any currently firing)
    /// 2. Process active jobs (reengage firing instances)
    /// 3. Reengage any firing session found in ContextStore
    pub fn on_startup(&self, alarms: &[(String, AlarmSpec)]) {
        // 1. Rearm all alarms (skip firing ones)
        let active_firing_id = self
            .current_session()
            .ok()
            .flatten()
            .map(|s| s.instance_id.0.clone());
        for (id_str, spec) in alarms {
            let id = InstanceId::new(id_str.clone());
            if Some(id_str) == active_firing_id.as_ref() {
                continue;
            }
            let res = if self.get(&id).is_ok() {
                self.update(&id, spec.clone())
            } else {
                self.create_with_id(id, spec.clone())
            };
            if let Err(e) = res {
                log::warn!("[AlarmKit] on_startup rearm failed for {}: {:?}", id_str, e);
            }
        }

        // 2. Process active jobs — for any active job with an
        //    instance_id payload, engage_or_fire (reengage if firing,
        //    dispatch Fire otherwise).
        if let Ok(jobs) = crate::jobs::get_active_jobs() {
            for job in &jobs {
                if let Some(instance_id_str) =
                    job.payload.get("instance_id").and_then(|v| v.as_str())
                {
                    self.engage_or_fire(&InstanceId::new(instance_id_str.to_owned()));
                }
            }
        }

        // 3. Reengage any firing session
        if let Ok(Some(session)) = self.current_session() {
            let _ = self.reengage(&session.instance_id);
        }
    }

    /// Static helper that detects the device timezone. On Android calls
    /// JNI, falls back to the `TZ` env var or `"UTC"`.
    pub fn detect_timezone() -> String {
        #[cfg(target_os = "android")]
        {
            let tz = crate::platform::android::firing_sink_android::get_time_zone_id();
            if !tz.is_empty() && tz != "UTC" {
                return tz;
            }
        }
        std::env::var("TZ").unwrap_or_else(|_| "UTC".to_owned())
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
                kiosk_hide_status_bar: false,
                kiosk_hide_nav_bar: false,
                firing_full_screen: true,
                vibration_pattern: vec![0, 400, 200, 400],
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

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
    /// Skip every occurrence at or before this instant (unix ms); see
    /// [`AlarmClassContext::skip_until_unix_ms`].
    pub skip_until_unix_ms: Option<i64>,
    /// Fade-in time of the alarm sound in seconds (0 = off).
    pub fade_in_secs: u16,
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

/// What [`AlarmKit::on_startup`] had to clean up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupReport {
    /// Alarms that were ringing (or snoozed past their time) when the device
    /// rebooted. They were dismissed; the consumer may want to tell the user
    /// and clear its own per-session files.
    pub ended_by_reboot: Vec<InstanceId>,
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
    /// re-arms with the new schedule. A snooze in progress is left alone
    /// when the spec did not change — consumers save every alarm whenever
    /// any one of them changes, and that must not cancel a snooze.
    pub fn update(&self, id: &InstanceId, spec: AlarmSpec) -> Result<(), AlarmKitError> {
        let mut record = self.store.load(id)?;
        if let RecipeContext::AlarmClass(old) = &record.context {
            if matches!(record.state, SessionState::Snoozed { .. })
                && AlarmSpec::from_context(old) == spec
            {
                return Ok(());
            }
        }
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
    /// If the alarm has challenges and is currently firing, this returns
    /// `Err(AlarmKitError::Recipe(RecipeError::ChallengesUnsolved(_)))` until
    /// [`Self::mark_solved`] has been called. The sentinel job is left
    /// active in that case.
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
    /// 1. End sessions that did not survive a reboot (see [`StartupReport`])
    /// 2. Rearm all provided alarms. An alarm whose spec is unchanged keeps
    ///    its state — in particular a snooze in progress — and only has its
    ///    exact alarm re-armed; a changed or new spec is (re)scheduled.
    /// 3. Process active jobs (engage the firing instances)
    /// 4. Reengage a firing session no active job covered
    pub fn on_startup(&self, alarms: &[(String, AlarmSpec)]) -> StartupReport {
        self.on_startup_at(
            alarms,
            crate::utilities::boot::boot_unix_ms(),
            Utc::now().timestamp_millis(),
        )
    }

    /// [`Self::on_startup`] with the boot time and the clock injected.
    fn on_startup_at(
        &self,
        alarms: &[(String, AlarmSpec)],
        boot_unix_ms: Option<i64>,
        now_ms: i64,
    ) -> StartupReport {
        let mut report = StartupReport::default();

        // 1. A reboot kills the process, the sound and the lock screen
        //    takeover, and clears every exact alarm. A ringing alarm from
        //    before the boot therefore counts as dismissed (it must not
        //    ring again when the app is merely opened), and so does a
        //    snooze whose time passed while the device was off. A snooze
        //    still in the future is kept and re-armed below.
        if let Some(boot) = boot_unix_ms {
            if let Ok(records) = self.store.enumerate_all() {
                for record in records {
                    let stale = match record.state {
                        SessionState::Firing { fired_at_unix_ms } => fired_at_unix_ms < boot,
                        SessionState::Snoozed {
                            next_fire_unix_ms, ..
                        } => next_fire_unix_ms <= now_ms,
                        _ => false,
                    };
                    if stale && self.end_after_reboot(&record.id) {
                        report.ended_by_reboot.push(record.id.clone());
                    }
                }
            }
        }

        // 2. Rearm the provided alarms.
        for (id_str, spec) in alarms {
            let id = InstanceId::new(id_str.clone());
            if let Err(e) = self.rearm(&id, spec, now_ms) {
                log::warn!("[AlarmKit] on_startup rearm failed for {}: {:?}", id_str, e);
            }
        }

        // 3. Process active jobs — for any active job with an
        //    instance_id payload, engage_or_fire (reengage if firing,
        //    dispatch Fire otherwise). Each instance is engaged once: every
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

        // 4. Reengage a firing session that no active job covered.
        if let Ok(Some(session)) = self.current_session() {
            if !engaged.contains(&session.instance_id.0) {
                let _ = self.reengage(&session.instance_id);
            }
        }
        report
    }

    /// Dismiss a session left over from before a reboot, bypassing the
    /// challenge gate (the user is not at the phone solving anything).
    /// Returns whether the instance was dismissed.
    fn end_after_reboot(&self, id: &InstanceId) -> bool {
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
                log::warn!("[AlarmKit] ending {} after reboot failed: {:?}", id.0, e);
                false
            }
        }
    }

    /// Make sure `id` is stored with `spec` and armed, without disturbing a
    /// firing session or a snooze in progress.
    fn rearm(&self, id: &InstanceId, spec: &AlarmSpec, now_ms: i64) -> Result<(), AlarmKitError> {
        let record = match self.store.load(id) {
            Ok(r) => r,
            Err(StoreError::NotFound(_)) => return self.create_with_id(id.clone(), spec.clone()),
            Err(e) => return Err(e.into()),
        };
        let RecipeContext::AlarmClass(ctx) = &record.context else {
            return self.create_with_id(id.clone(), spec.clone());
        };
        if matches!(record.state, SessionState::Firing { .. }) {
            return Ok(());
        }
        if AlarmSpec::from_context(ctx) != *spec {
            return self.update(id, spec.clone());
        }
        match record.state {
            // The exact alarm was cleared by the reboot; the stored fire
            // time is still the right one.
            SessionState::Snoozed {
                next_fire_unix_ms, ..
            } if next_fire_unix_ms > now_ms => {
                if let Some(sink) = alarm_class_runtime().sink() {
                    let req = crate::firing::ExactAlarmRequest {
                        alarm_id: id.0.clone(),
                        target_unix_ms: next_fire_unix_ms,
                        metadata_json: None,
                    };
                    if !sink.schedule_exact_alarm(&req) {
                        log::warn!("[AlarmKit] re-arming snooze of {} was rejected", id.0);
                    }
                }
                Ok(())
            }
            SessionState::Snoozed { .. } => Ok(()),
            // Scheduled / Idle / Dismissed: recompute and arm the next fire.
            _ => {
                dispatch_trigger(&self.store, Trigger::Schedule, id)?;
                Ok(())
            }
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
                min_volume_percent: 0,
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

    #[test]
    fn on_startup_ends_a_snooze_whose_time_passed_while_the_device_was_off() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let (kit, _sink) = install_kit(dir.path());
        let now = 2_000_000_000_000_i64;
        let spec = weekday_spec();
        let id = kit.create(spec.clone()).unwrap();
        force_state(
            &kit,
            &id,
            SessionState::Snoozed {
                snooze_count: 1,
                next_fire_unix_ms: now - 10_000,
            },
        );

        let report = kit.on_startup_at(&[(id.0.clone(), spec)], Some(now - 60_000), now);

        assert_eq!(report.ended_by_reboot, vec![id.clone()]);
        assert!(matches!(
            kit.store().load(&id).unwrap().state,
            SessionState::Scheduled { .. }
        ));
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

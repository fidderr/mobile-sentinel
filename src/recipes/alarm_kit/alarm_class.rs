//! AlarmClass — generic scheduled-fire Recipe with snooze, dismiss,
//! optional challenge gate, and sound selection.
//!
//! AlarmClass is intentionally generic. Any consumer app — alarm clocks,
//! task reminders, fitness reminders — can use it without source forks.
//! Challenge gating happens at the UI layer (consumer side); AlarmClass
//! itself just runs the state machine and writes ContextStore.

use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::recipes::context::schema::{
    AlarmClassContext, ContextRecord, RecipeContext, SessionState,
};
use crate::recipes::context::{ContextStore, StoreError};
use crate::recipes::recipe::{Recipe, RecipeError, RecipePermission};
use crate::recipes::recurrence::{next_fire, Schedule};
use crate::recipes::trigger::Trigger;
use crate::types::InstanceId;

/// AlarmClass Recipe.
pub struct AlarmClass;

impl AlarmClass {
    /// Construct an AlarmClass Recipe ready for [`crate::recipes::register_recipe`].
    pub fn new() -> Self {
        Self
    }
}

impl Default for AlarmClass {
    fn default() -> Self {
        Self::new()
    }
}

const PERMISSIONS: &[RecipePermission] = &[
    RecipePermission {
        name: "android.permission.SCHEDULE_EXACT_ALARM",
        rationale: "Schedule alarms at exact times",
        required: true,
        min_api: Some(31),
    },
    RecipePermission {
        name: "android.permission.USE_EXACT_ALARM",
        rationale: "Use exact alarms (API 33+)",
        required: true,
        min_api: Some(33),
    },
    RecipePermission {
        name: "android.permission.POST_NOTIFICATIONS",
        rationale: "Show alarm notifications",
        required: false,
        min_api: Some(33),
    },
    RecipePermission {
        name: "android.permission.SYSTEM_ALERT_WINDOW",
        rationale: "Show alarm over other apps",
        required: false,
        min_api: None,
    },
    RecipePermission {
        name: "android.permission.RECEIVE_BOOT_COMPLETED",
        rationale: "Re-arm alarms after reboot",
        required: true,
        min_api: None,
    },
    RecipePermission {
        name: "android.permission.FOREGROUND_SERVICE",
        rationale: "Keep alarm alive in background",
        required: true,
        min_api: Some(28),
    },
    RecipePermission {
        name: "android.permission.USE_FULL_SCREEN_INTENT",
        rationale: "Show full-screen alarm on lock screen",
        required: true,
        min_api: Some(34),
    },
    RecipePermission {
        name: "android.permission.WAKE_LOCK",
        rationale: "Keep device awake during alarm",
        required: true,
        min_api: None,
    },
    RecipePermission {
        name: "android.permission.VIBRATE",
        rationale: "Vibrate while the alarm fires",
        required: false,
        min_api: None,
    },
];

impl Recipe for AlarmClass {
    fn recipe_type(&self) -> &'static str {
        "alarm_class"
    }

    fn required_permissions(&self) -> &'static [RecipePermission] {
        PERMISSIONS
    }

    fn handle_trigger(
        &self,
        trigger: Trigger,
        instance_id: &InstanceId,
        context: &RecipeContext,
    ) -> Result<(), RecipeError> {
        let _alarm_ctx = expect_alarm_class(context)?;
        // Reach the global ContextStore via the runtime hook so write-back
        // is integrated. The runtime sets this once during init; tests
        // pass their own store via `handle_trigger_with_store`.
        let store = match alarm_class_runtime().store() {
            Some(s) => s,
            None => {
                // No store wired — the dispatch layer is being driven in
                // test mode without a runtime hook. Recipe handlers run
                // with a Context already loaded by the caller; no write
                // is attempted, which preserves LoadContext-on-trigger
                // (the next dispatch will re-read).
                return Ok(());
            }
        };
        handle_trigger_with_store(&store, trigger, instance_id, context)
    }
}

/// Execute the AlarmClass `handle_trigger` flow against an explicit
/// store. This is the actual state-machine executor that
/// [`AlarmClass::handle_trigger`] delegates to once it has resolved the
/// global store. Consumers drive dispatch through
/// [`crate::recipes::dispatch_trigger`] (the universal path); this
/// function is exposed for AlarmClass-internal use and for tests that
/// want to drive a deterministic dispatch against a temp store.
pub fn handle_trigger_with_store(
    store: &ContextStore,
    trigger: Trigger,
    instance_id: &InstanceId,
    context: &RecipeContext,
) -> Result<(), RecipeError> {
    let alarm_ctx = expect_alarm_class(context)?;
    match trigger {
        Trigger::Schedule => schedule_next(store, instance_id, alarm_ctx, Utc::now()),
        Trigger::Edit => {
            // The caller has already overwritten the Context — we only
            // need to re-arm the next fire with the new schedule.
            schedule_next(store, instance_id, alarm_ctx, Utc::now())
        }
        Trigger::Fire => fire(store, instance_id, alarm_ctx, Utc::now()),
        Trigger::Snooze => snooze(store, instance_id, alarm_ctx, Utc::now()),
        Trigger::Dismiss => dismiss(store, instance_id, alarm_ctx, Utc::now()),
        Trigger::Solve => mark_challenges_solved(store, instance_id, alarm_ctx),
        Trigger::Pause => {
            // Pause the firing audio without tearing down other surfaces
            // (e.g. the user entered a challenge screen). State is unchanged.
            if let Some(sink) = alarm_class_runtime().sink() {
                sink.pause_audio();
            }
            Ok(())
        }
        Trigger::Resume => resume(store, instance_id, alarm_ctx),
    }
}

/// Compute the next fire time and persist `Scheduled { next_fire_unix_ms }`.
/// If the record is currently `Firing` we never overwrite the state — a
/// concurrent re-arm (e.g. MAIN cold-starting via FSI while `:sentinel`
/// is mid-fire) must not blow away the active session. The sink-level
/// re-arm of the next exact alarm still happens so a future schedule
/// has a wake registered.
fn schedule_next(
    store: &ContextStore,
    instance_id: &InstanceId,
    alarm_ctx: &AlarmClassContext,
    now: DateTime<Utc>,
) -> Result<(), RecipeError> {
    let schedule = parse_schedule(alarm_ctx)?;
    let zone = parse_zone(alarm_ctx)?;
    let next = next_fire(&schedule, &zone, now)?;

    let mut record = match store.load(instance_id) {
        Ok(r) => r,
        Err(StoreError::NotFound(_)) => {
            ContextRecord::new(instance_id.clone(), context_clone(alarm_ctx))
        }
        Err(e) => return Err(RecipeError::Store(e)),
    };
    let was_firing = matches!(record.state, SessionState::Firing { .. });
    let was_snoozed = matches!(record.state, SessionState::Snoozed { .. });
    if !was_firing {
        // Reset the snooze count on (re)schedule so a fresh Firing
        // session starts from zero. Do NOT touch a Snoozed record's
        // counter — the user is mid-snooze cycle and the count must
        // survive the re-arm.
        if !was_snoozed {
            record.context = reset_snooze(alarm_ctx);
        }
        record.state = SessionState::Scheduled {
            next_fire_unix_ms: next.timestamp_millis(),
        };
        store.write(record).map_err(RecipeError::Store)?;
    }

    // Arm an exact alarm so the OS wakes :sentinel at next_fire even if
    // the app process is fully gone. Best-effort: a missing sink (host
    // builds, tests) leaves the record persisted without arming.
    // For Firing records we still arm the next fire so the recurring
    // alarm has a wake registered for tomorrow / next-occurrence.
    if let Some(sink) = alarm_class_runtime().sink() {
        if !was_firing && !was_snoozed {
            sink.cancel_exact_alarm(&instance_id.0);
            let req = crate::firing::ExactAlarmRequest {
                alarm_id: instance_id.0.clone(),
                target_unix_ms: next.timestamp_millis(),
                metadata_json: None,
            };
            if !sink.schedule_exact_alarm(&req) {
                log::warn!(
                    "[AlarmClass] schedule_exact_alarm rejected for {}",
                    instance_id.0
                );
            }
        } else {
            log::info!(
                "[AlarmClass] schedule_next: skipping re-arm because instance {} is in active state",
                instance_id.0
            );
        }
    }
    Ok(())
}

fn fire(
    store: &ContextStore,
    instance_id: &InstanceId,
    alarm_ctx: &AlarmClassContext,
    now: DateTime<Utc>,
) -> Result<(), RecipeError> {
    let mut record = load_or_init(store, instance_id, alarm_ctx)?;

    // Idempotent: if already firing, don't re-engage surfaces (avoids
    // restarting audio on every guardian heads-up poll).
    if matches!(record.state, SessionState::Firing { .. }) {
        return Ok(());
    }

    record.state = SessionState::Firing {
        fired_at_unix_ms: now.timestamp_millis(),
    };
    // Reset challenges_solved at the start of every Firing session.
    if let RecipeContext::AlarmClass(ctx) = &mut record.context {
        ctx.challenges_solved = false;
    }
    store.write(record).map_err(RecipeError::Store)?;

    // Engage every "alarm is firing" platform surface.
    engage_firing_surface(instance_id, alarm_ctx);
    Ok(())
}

/// Re-engage all firing surfaces for an instance already in `Firing` state
/// without changing state. This is the canonical "bring the alarm back"
/// path — used when MAIN restarts mid-fire, and when a paused session
/// (e.g. a challenge screen) resumes. A no-op if the instance is not
/// currently `Firing`.
fn resume(
    store: &ContextStore,
    instance_id: &InstanceId,
    alarm_ctx: &AlarmClassContext,
) -> Result<(), RecipeError> {
    let record = store.load(instance_id).map_err(RecipeError::Store)?;
    if !matches!(record.state, SessionState::Firing { .. }) {
        // Nothing firing — resume is a no-op (matches Pause being state-free).
        return Ok(());
    }
    engage_firing_surface(instance_id, alarm_ctx);
    Ok(())
}

/// Build the [`crate::firing::FireRequest`] for an instance from the current
/// runtime config + the instance's Context, and drive the installed sink's
/// `start_firing`. The single place a `FireRequest` is constructed, so `fire`
/// and `resume` (and AlarmKit's re-engage on cold start) never drift apart.
/// Sound is resolved through the registered `SoundResolver`, falling back to
/// the sink's system default. Best-effort: a missing sink (host builds,
/// tests) is a silent no-op; the sink owns retries / failure logging.
fn engage_firing_surface(instance_id: &InstanceId, alarm_ctx: &AlarmClassContext) {
    if let Some(sink) = alarm_class_runtime().sink() {
        let runtime_cfg = alarm_class_runtime().config();
        let sound_uri = match alarm_class_runtime().sound_resolver() {
            Some(r) => r.resolve(&alarm_ctx.sound_id),
            None => sink.system_default_sound_uri(),
        };
        let req = crate::firing::FireRequest {
            instance_id: instance_id.0.clone(),
            channel_id: runtime_cfg.channel_id.clone(),
            channel_name: runtime_cfg.channel_name.clone(),
            title: alarm_ctx.label.clone(),
            body: runtime_cfg.firing_body.clone(),
            importance: runtime_cfg.importance,
            bypass_dnd: alarm_ctx.bypass_dnd,
            sound_uri,
            audio_usage: runtime_cfg.audio_usage.clone(),
            audio_content_type: runtime_cfg.audio_content_type.clone(),
            looping: true,
            // Vibration is the instance's choice (`vibration_enabled`); the
            // waveform follows the three-level fallback — per-alarm override
            // (`alarm_ctx.vibration_pattern`) wins, else the consumer's app
            // default (`runtime_cfg.vibration_pattern`), else (if both empty)
            // the firing sink's built-in pattern.
            vibrate: alarm_ctx.vibration_enabled,
            vibration_pattern: alarm_ctx
                .vibration_pattern
                .clone()
                .unwrap_or_else(|| runtime_cfg.vibration_pattern.clone()),
            kiosk_mode: alarm_ctx.kiosk_mode,
            kiosk_block_home: runtime_cfg.kiosk_block_home,
            kiosk_block_back: runtime_cfg.kiosk_block_back,
            kiosk_block_recents: runtime_cfg.kiosk_block_recents,
            activity_fqcn: runtime_cfg.activity_fqcn.clone(),
            kiosk_debounce_ms: runtime_cfg.kiosk_debounce_ms,
            kiosk_hide_status_bar: runtime_cfg.kiosk_hide_status_bar,
            kiosk_hide_nav_bar: runtime_cfg.kiosk_hide_nav_bar,
            firing_full_screen: runtime_cfg.firing_full_screen,
        };
        if !sink.start_firing(&req) {
            log::warn!(
                "[AlarmClass] start_firing reported failure for {}",
                instance_id.0
            );
        }
    }
}

fn snooze(
    store: &ContextStore,
    instance_id: &InstanceId,
    alarm_ctx: &AlarmClassContext,
    now: DateTime<Utc>,
) -> Result<(), RecipeError> {
    let mut record = store.load(instance_id).map_err(RecipeError::Store)?;
    let policy = &alarm_ctx.snooze_policy;
    let current_count = alarm_ctx.snooze_count;
    let interval = match policy.interval_for(current_count) {
        Some(i) => i,
        None => {
            // Cap reached — auto-dismiss.
            return dismiss(store, instance_id, alarm_ctx, now);
        }
    };
    let next_unix_ms = now.timestamp_millis() + (interval as i64) * 60_000;
    if let RecipeContext::AlarmClass(ctx) = &mut record.context {
        ctx.snooze_count = current_count + 1;
        ctx.challenges_solved = false;
    }
    record.state = SessionState::Snoozed {
        snooze_count: current_count + 1,
        next_fire_unix_ms: next_unix_ms,
    };
    store.write(record).map_err(RecipeError::Store)?;

    // Tear down the firing surface and re-arm the next exact alarm so
    // the snooze re-fires even if the app gets killed in the meantime.
    if let Some(sink) = alarm_class_runtime().sink() {
        sink.stop_firing(&instance_id.0);
        sink.cancel_exact_alarm(&instance_id.0);
        let req = crate::firing::ExactAlarmRequest {
            alarm_id: instance_id.0.clone(),
            target_unix_ms: next_unix_ms,
            metadata_json: None,
        };
        let _ = sink.schedule_exact_alarm(&req);
    }
    Ok(())
}

/// Transition an alarm session to `Dismissed` and persist it. Called when
/// the consumer satisfies the dismiss conditions (or the snooze cap is
/// reached). Idempotent with respect to the stored session record.
fn dismiss(
    store: &ContextStore,
    instance_id: &InstanceId,
    alarm_ctx: &AlarmClassContext,
    now: DateTime<Utc>,
) -> Result<(), RecipeError> {
    let mut record = store.load(instance_id).map_err(RecipeError::Store)?;
    record.state = SessionState::Dismissed {
        dismissed_at_unix_ms: now.timestamp_millis(),
    };
    if let RecipeContext::AlarmClass(ctx) = &mut record.context {
        ctx.snooze_count = 0;
        ctx.challenges_solved = false;
    }
    store.write(record).map_err(RecipeError::Store)?;

    // Tear down everything fire engaged.
    if let Some(sink) = alarm_class_runtime().sink() {
        sink.stop_firing(&instance_id.0);
        sink.cancel_exact_alarm(&instance_id.0);
    }

    // For recurring schedules, transition Dismissed → Scheduled with the
    // next fire time. One-shot schedules go to Idle (handled below).
    let schedule = parse_schedule(alarm_ctx)?;
    let zone = parse_zone(alarm_ctx)?;
    let advance_now = now + chrono::Duration::seconds(1);
    match next_fire(&schedule, &zone, advance_now) {
        Ok(next) => {
            let mut next_record = store.load(instance_id).map_err(RecipeError::Store)?;
            next_record.state = SessionState::Scheduled {
                next_fire_unix_ms: next.timestamp_millis(),
            };
            store.write(next_record).map_err(RecipeError::Store)?;
            // Re-arm the next exact alarm.
            if let Some(sink) = alarm_class_runtime().sink() {
                let req = crate::firing::ExactAlarmRequest {
                    alarm_id: instance_id.0.clone(),
                    target_unix_ms: next.timestamp_millis(),
                    metadata_json: None,
                };
                let _ = sink.schedule_exact_alarm(&req);
            }
        }
        Err(_) => {
            // No next fire — leave in Dismissed; one-shot completed.
        }
    }
    Ok(())
}

fn mark_challenges_solved(
    store: &ContextStore,
    instance_id: &InstanceId,
    _alarm_ctx: &AlarmClassContext,
) -> Result<(), RecipeError> {
    let mut record = store.load(instance_id).map_err(RecipeError::Store)?;
    if let RecipeContext::AlarmClass(ctx) = &mut record.context {
        ctx.challenges_solved = true;
    }
    store.write(record).map_err(RecipeError::Store)?;
    Ok(())
}

fn load_or_init(
    store: &ContextStore,
    instance_id: &InstanceId,
    alarm_ctx: &AlarmClassContext,
) -> Result<ContextRecord, RecipeError> {
    match store.load(instance_id) {
        Ok(r) => Ok(r),
        Err(StoreError::NotFound(_)) => Ok(ContextRecord::new(
            instance_id.clone(),
            context_clone(alarm_ctx),
        )),
        Err(e) => Err(RecipeError::Store(e)),
    }
}

fn parse_schedule(alarm_ctx: &AlarmClassContext) -> Result<Schedule, RecipeError> {
    Schedule::try_from(&alarm_ctx.schedule).map_err(|e| RecipeError::InvalidContext {
        recipe: "alarm_class",
        message: format!("schedule: {e}"),
    })
}

fn parse_zone(alarm_ctx: &AlarmClassContext) -> Result<chrono_tz::Tz, RecipeError> {
    alarm_ctx
        .time_zone
        .parse::<chrono_tz::Tz>()
        .map_err(|e| RecipeError::InvalidContext {
            recipe: "alarm_class",
            message: format!("time_zone '{}': {}", alarm_ctx.time_zone, e),
        })
}

fn expect_alarm_class(context: &RecipeContext) -> Result<&AlarmClassContext, RecipeError> {
    match context {
        RecipeContext::AlarmClass(ctx) => Ok(ctx),
        other => Err(RecipeError::InvalidContext {
            recipe: "alarm_class",
            message: format!("expected alarm_class context, got {}", other.recipe_type()),
        }),
    }
}

fn context_clone(ctx: &AlarmClassContext) -> RecipeContext {
    RecipeContext::AlarmClass(ctx.clone())
}

fn reset_snooze(ctx: &AlarmClassContext) -> RecipeContext {
    let mut c = ctx.clone();
    c.snooze_count = 0;
    c.challenges_solved = false;
    RecipeContext::AlarmClass(c)
}

// ---------------------------------------------------------------------------
// AlarmClass runtime hook — wires the Recipe to the global ContextStore.
// ---------------------------------------------------------------------------

/// Notification-channel + activity wiring that AlarmClass hands to the
/// [`crate::FiringSink`] at fire time. Consumers configure these once during
/// init via [`AlarmClassRuntime::configure`]; defaults are reasonable
/// for an alarm-clock product but can be overridden per consumer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlarmClassConfig {
    /// Notification channel id used by the firing FGS.
    pub channel_id: String,
    /// Display name for the firing channel (used on first-create only).
    pub channel_name: String,
    /// Notification body shown while firing.
    pub firing_body: String,
    /// Channel importance — 5 is `IMPORTANCE_MAX`.
    pub importance: i32,
    /// AudioAttributes usage tag.
    pub audio_usage: String,
    /// AudioAttributes content type.
    pub audio_content_type: String,
    /// Activity FQCN for full-screen-intent + kiosk relaunch. `None`
    /// disables both.
    pub activity_fqcn: Option<String>,
    /// Debounce (ms) for kiosk relaunch on user backgrounding.
    pub kiosk_debounce_ms: u32,
    /// Kiosk: relaunch the activity when the user presses HOME.
    pub kiosk_block_home: bool,
    /// Kiosk: consume the BACK gesture.
    pub kiosk_block_back: bool,
    /// Kiosk: relaunch the activity when the user opens Recents / swipes away.
    pub kiosk_block_recents: bool,
    /// While kiosk-firing, hide the status bar (clock / battery / signal).
    /// `false` keeps the system status bar visible during the alarm.
    pub kiosk_hide_status_bar: bool,
    /// While kiosk-firing, hide the navigation bar (home / back / recents
    /// gesture area). `true` reinforces the lockdown.
    pub kiosk_hide_nav_bar: bool,
    /// Use a full-screen-intent notification while firing. When `true`
    /// the alarm can wake a locked/asleep screen and show the UI over the
    /// keyguard. The platform automatically suppresses the on-screen
    /// heads-up banner when the firing activity is *already* in the
    /// foreground (the UI is visible, so the banner is redundant). Set
    /// `false` to never use a full-screen intent (quiet tray notification
    /// only — note this may not reliably wake a locked screen).
    pub firing_full_screen: bool,
    /// Vibration waveform (alternating wait/vibrate ms) looped while firing,
    /// for instances whose `vibration_enabled` is set. The consumer's
    /// default-or-override knob; the default is an alarm-style buzz. An empty
    /// vec lets the firing sink fall back to its own built-in pattern.
    pub vibration_pattern: Vec<i64>,
}

impl Default for AlarmClassConfig {
    fn default() -> Self {
        Self {
            channel_id: "alarmclass_firing".into(),
            channel_name: "Alarms".into(),
            firing_body: "Tap to open".into(),
            importance: 5,
            audio_usage: "alarm".into(),
            audio_content_type: "sonification".into(),
            activity_fqcn: None,
            kiosk_debounce_ms: 50,
            kiosk_block_home: true,
            kiosk_block_back: true,
            kiosk_block_recents: true,
            kiosk_hide_status_bar: false,
            kiosk_hide_nav_bar: false,
            firing_full_screen: true,
            vibration_pattern: vec![0, 400, 200, 400],
        }
    }
}

/// Runtime wiring for AlarmClass. The consumer's init code calls
/// [`AlarmClassRuntime::set_store`] once during startup so the Recipe's
/// `handle_trigger` can write back transitions. In tests the runtime is
/// left unwired and tests call [`handle_trigger_with_store`] directly.
pub struct AlarmClassRuntime {
    store: std::sync::RwLock<Option<Arc<ContextStore>>>,
    sink: std::sync::RwLock<Option<Arc<dyn crate::firing::FiringSink>>>,
    sound_lib: std::sync::RwLock<Option<Arc<dyn SoundResolver>>>,
    config: std::sync::RwLock<AlarmClassConfig>,
}

/// Trait-erased sound resolver — the AlarmClass runtime uses this to
/// turn a [`crate::recipes::context::schema::SoundIdSpec`] into a playable URI
/// without knowing about the concrete `SoundLibrary<B>` generic.
pub trait SoundResolver: Send + Sync {
    fn resolve(&self, id: &crate::recipes::context::schema::SoundIdSpec) -> String;
}

// Bridge the recipe's `SoundIdSpec` to the `sound` feature's standalone
// `SoundId` + `SoundLibrary`. This adapter lives in the RECIPE layer (which
// is allowed to depend on features) — the `sound` feature itself knows
// nothing about recipe context types. `sound-library` is always enabled with
// `alarm-kit`.
#[cfg(feature = "sound-library")]
impl<B: crate::sound::SoundBackend + 'static> SoundResolver for crate::sound::SoundLibrary<B> {
    fn resolve(&self, id: &crate::recipes::context::schema::SoundIdSpec) -> String {
        crate::sound::SoundLibrary::resolve(self, &sound_id_spec_to_sound_id(id))
    }
}

/// Map the recipe context's [`SoundIdSpec`](crate::recipes::context::schema::SoundIdSpec)
/// onto the `sound` feature's standalone [`SoundId`](crate::sound::SoundId).
/// The mapping lives here (recipe side), keeping the `sound` feature
/// recipe-agnostic.
#[cfg(feature = "sound-library")]
fn sound_id_spec_to_sound_id(
    spec: &crate::recipes::context::schema::SoundIdSpec,
) -> crate::sound::SoundId {
    use crate::recipes::context::schema::SoundIdSpec;
    use crate::sound::SoundId;
    match spec {
        SoundIdSpec::Bundled(s) => SoundId::Bundled(s.clone()),
        SoundIdSpec::Custom(s) => SoundId::Custom(s.clone()),
        SoundIdSpec::SystemDefault => SoundId::SystemDefault,
        SoundIdSpec::Silent => SoundId::Silent,
    }
}

impl AlarmClassRuntime {
    pub fn store(&self) -> Option<Arc<ContextStore>> {
        self.store.read().unwrap().clone()
    }

    pub fn set_store(&self, store: Arc<ContextStore>) {
        *self.store.write().unwrap() = Some(store);
    }

    /// Borrow the installed FiringSink, if any.
    pub fn sink(&self) -> Option<Arc<dyn crate::firing::FiringSink>> {
        self.sink.read().unwrap().clone()
    }

    /// Install the platform FiringSink. Last call wins.
    pub fn set_sink(&self, sink: Arc<dyn crate::firing::FiringSink>) {
        *self.sink.write().unwrap() = Some(sink);
    }

    /// Borrow the installed sound resolver, if any.
    pub fn sound_resolver(&self) -> Option<Arc<dyn SoundResolver>> {
        self.sound_lib.read().unwrap().clone()
    }

    pub fn set_sound_resolver(&self, resolver: Arc<dyn SoundResolver>) {
        *self.sound_lib.write().unwrap() = Some(resolver);
    }

    /// Read the current AlarmClass config snapshot.
    pub fn config(&self) -> AlarmClassConfig {
        self.config.read().unwrap().clone()
    }

    /// Override the AlarmClass runtime config (channel ids, activity, etc.).
    pub fn configure(&self, config: AlarmClassConfig) {
        *self.config.write().unwrap() = config;
    }
}

static RUNTIME: AlarmClassRuntime = AlarmClassRuntime {
    store: std::sync::RwLock::new(None),
    sink: std::sync::RwLock::new(None),
    sound_lib: std::sync::RwLock::new(None),
    config: std::sync::RwLock::new(AlarmClassConfig::const_default()),
};

impl AlarmClassConfig {
    /// Const-default used to initialise the static `RUNTIME`. The
    /// runtime is overwritten via [`AlarmClassRuntime::configure`] before
    /// any production fire path; this is just a placeholder.
    const fn const_default() -> Self {
        Self {
            channel_id: String::new(),
            channel_name: String::new(),
            firing_body: String::new(),
            importance: 5,
            audio_usage: String::new(),
            audio_content_type: String::new(),
            activity_fqcn: None,
            kiosk_debounce_ms: 50,
            kiosk_block_home: true,
            kiosk_block_back: true,
            kiosk_block_recents: true,
            kiosk_hide_status_bar: false,
            kiosk_hide_nav_bar: false,
            firing_full_screen: true,
            vibration_pattern: Vec::new(),
        }
    }
}

/// Borrow the global AlarmClass runtime.
pub fn alarm_class_runtime() -> &'static AlarmClassRuntime {
    &RUNTIME
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipes::context::schema::{
        AlarmClassContext, ScheduleSpec, SnoozePolicy, SoundIdSpec,
    };
    use crate::recipes::context::ContextStore;
    use crate::recipes::registry::register_recipe;
    use crate::recipes::test_lock::lock_and_clear;
    use chrono::TimeZone;
    use tempfile::TempDir;

    fn alarm_ctx() -> AlarmClassContext {
        AlarmClassContext {
            label: "Test".into(),
            schedule: ScheduleSpec::Weekdays {
                days_mask: 0b0001_1111,
                hour: 7,
                minute: 0,
            },
            time_zone: "UTC".into(),
            sound_id: SoundIdSpec::Bundled("happy".into()),
            snooze_policy: SnoozePolicy {
                max_count: 3,
                interval_minutes: 5,
                escalation: None,
            },
            challenges: vec![],
            vibration_enabled: false,
            vibration_pattern: None,
            kiosk_mode: false,
            bypass_dnd: false,
            snooze_count: 0,
            challenges_solved: false,
        }
    }

    fn sample_record(id: &str, ctx: AlarmClassContext) -> ContextRecord {
        ContextRecord::new(InstanceId::new(id), RecipeContext::AlarmClass(ctx))
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 8, 12, 0, 0).single().unwrap()
    }

    #[test]
    fn schedule_writes_scheduled_state_with_next_fire() {
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        store.write(sample_record("a", alarm_ctx())).unwrap();
        schedule_next(&store, &id, &alarm_ctx(), now()).unwrap();

        let r = store.load(&id).unwrap();
        match r.state {
            SessionState::Scheduled { next_fire_unix_ms } => {
                // 2026-06-08 is a Mon; next Mon@07:00 UTC is 2026-06-15 07:00.
                assert!(next_fire_unix_ms > now().timestamp_millis());
            }
            other => panic!("expected Scheduled, got {other:?}"),
        }
    }

    #[test]
    fn fire_writes_firing_state_and_resets_solved_flag() {
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        let mut ctx = alarm_ctx();
        ctx.challenges_solved = true; // Should be reset by fire.
        store.write(sample_record("a", ctx.clone())).unwrap();
        fire(&store, &id, &ctx, now()).unwrap();
        let r = store.load(&id).unwrap();
        assert!(matches!(r.state, SessionState::Firing { .. }));
        if let RecipeContext::AlarmClass(c) = &r.context {
            assert!(!c.challenges_solved);
        }
    }

    #[test]
    fn snooze_increments_count_and_persists_snoozed_state() {
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        let mut ctx = alarm_ctx();
        ctx.snooze_count = 0;
        store.write(sample_record("a", ctx.clone())).unwrap();
        snooze(&store, &id, &ctx, now()).unwrap();

        let r = store.load(&id).unwrap();
        if let RecipeContext::AlarmClass(c) = &r.context {
            assert_eq!(c.snooze_count, 1);
        }
        match r.state {
            SessionState::Snoozed {
                snooze_count,
                next_fire_unix_ms,
            } => {
                assert_eq!(snooze_count, 1);
                assert_eq!(next_fire_unix_ms, now().timestamp_millis() + 5 * 60 * 1000);
            }
            other => panic!("expected Snoozed, got {other:?}"),
        }
    }

    #[test]
    fn snooze_at_cap_dismisses_and_reschedules() {
        let _g = lock_and_clear();
        register_recipe(AlarmClass::new()).unwrap();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        let mut ctx = alarm_ctx();
        ctx.snooze_count = ctx.snooze_policy.max_count; // already at cap
        store.write(sample_record("a", ctx.clone())).unwrap();
        snooze(&store, &id, &ctx, now()).unwrap();

        let r = store.load(&id).unwrap();
        // Recurring schedule → next fire is rearmed as Scheduled.
        assert!(matches!(r.state, SessionState::Scheduled { .. }));
    }

    #[test]
    fn dismiss_proceeds_to_dismissed_or_rescheduled() {
        let _g = lock_and_clear();
        register_recipe(AlarmClass::new()).unwrap();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        let ctx = alarm_ctx();
        let mut record = sample_record("a", ctx.clone());
        record.state = SessionState::Firing {
            fired_at_unix_ms: 0,
        };
        store.write(record).unwrap();

        dismiss(&store, &id, &ctx, now()).unwrap();
        let r = store.load(&id).unwrap();
        // Recurring weekday schedule re-arms to Scheduled.
        assert!(matches!(r.state, SessionState::Scheduled { .. }));
        if let RecipeContext::AlarmClass(c) = &r.context {
            assert_eq!(c.snooze_count, 0);
        }
    }

    #[test]
    fn solve_marks_challenges_solved() {
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        let ctx = alarm_ctx();
        let mut record = sample_record("a", ctx.clone());
        record.state = SessionState::Firing {
            fired_at_unix_ms: 0,
        };
        store.write(record).unwrap();
        mark_challenges_solved(&store, &id, &ctx).unwrap();
        let r = store.load(&id).unwrap();
        if let RecipeContext::AlarmClass(c) = &r.context {
            assert!(c.challenges_solved);
        }
    }

    #[test]
    fn handle_trigger_with_store_dispatches_correctly() {
        let _g = lock_and_clear();
        register_recipe(AlarmClass::new()).unwrap();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        store.write(sample_record("a", alarm_ctx())).unwrap();

        // Schedule
        handle_trigger_with_store(
            &store,
            Trigger::Schedule,
            &id,
            &RecipeContext::AlarmClass(alarm_ctx()),
        )
        .unwrap();
        assert!(matches!(
            store.load(&id).unwrap().state,
            SessionState::Scheduled { .. }
        ));

        // Fire
        handle_trigger_with_store(
            &store,
            Trigger::Fire,
            &id,
            &RecipeContext::AlarmClass(alarm_ctx()),
        )
        .unwrap();
        assert!(matches!(
            store.load(&id).unwrap().state,
            SessionState::Firing { .. }
        ));
    }

    #[test]
    fn metadata_flow_property_scheduled_settings_observed_at_fire_time() {
        // The property from Requirement 22.2: settings present in the
        // Context at scheduling time MUST be present at fire time.
        let _g = lock_and_clear();
        register_recipe(AlarmClass::new()).unwrap();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        let mut ctx = alarm_ctx();
        ctx.label = "Specific Label".into();
        ctx.sound_id = SoundIdSpec::Bundled("specific_sound".into());
        store.write(sample_record("a", ctx.clone())).unwrap();

        handle_trigger_with_store(
            &store,
            Trigger::Schedule,
            &id,
            &RecipeContext::AlarmClass(ctx.clone()),
        )
        .unwrap();
        let after_schedule = store.load(&id).unwrap();
        if let RecipeContext::AlarmClass(c) = &after_schedule.context {
            assert_eq!(c.label, "Specific Label");
            assert!(matches!(&c.sound_id, SoundIdSpec::Bundled(s) if s == "specific_sound"));
        }

        handle_trigger_with_store(&store, Trigger::Fire, &id, &RecipeContext::AlarmClass(ctx))
            .unwrap();
        let after_fire = store.load(&id).unwrap();
        if let RecipeContext::AlarmClass(c) = &after_fire.context {
            assert_eq!(c.label, "Specific Label");
            assert!(matches!(&c.sound_id, SoundIdSpec::Bundled(s) if s == "specific_sound"));
        }
    }

    #[test]
    fn snooze_count_monotonicity_property() {
        // Requirement 11.6 / 22.4 — snooze count strictly monotonic.
        let _g = lock_and_clear();
        register_recipe(AlarmClass::new()).unwrap();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        let ctx = alarm_ctx();
        store.write(sample_record("a", ctx.clone())).unwrap();

        let mut prev_count = 0;
        for i in 1..=ctx.snooze_policy.max_count {
            // Reload context each time — that's the LoadContext-on-trigger
            // discipline.
            let current = store.load(&id).unwrap();
            let alarm = match &current.context {
                RecipeContext::AlarmClass(c) => c.clone(),
                _ => panic!(),
            };
            snooze(&store, &id, &alarm, now()).unwrap();
            let after = store.load(&id).unwrap();
            if let RecipeContext::AlarmClass(c) = &after.context {
                assert_eq!(c.snooze_count, i);
                assert!(c.snooze_count > prev_count);
                prev_count = c.snooze_count;
            }
        }
    }

    #[test]
    fn invalid_timezone_is_rejected() {
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        let mut ctx = alarm_ctx();
        ctx.time_zone = "Not/A/Real/Zone".into();
        store.write(sample_record("a", ctx.clone())).unwrap();
        let err = schedule_next(&store, &id, &ctx, now()).unwrap_err();
        assert!(matches!(err, RecipeError::InvalidContext { .. }));
    }

    #[test]
    fn fire_idempotency_property() {
        // Requirement 22.1 — Fire twice must produce the same Context state.
        let _g = lock_and_clear();
        register_recipe(AlarmClass::new()).unwrap();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        let ctx = alarm_ctx();
        store.write(sample_record("a", ctx.clone())).unwrap();

        // Pre-stage as Scheduled.
        handle_trigger_with_store(
            &store,
            Trigger::Schedule,
            &id,
            &RecipeContext::AlarmClass(ctx.clone()),
        )
        .unwrap();

        handle_trigger_with_store(
            &store,
            Trigger::Fire,
            &id,
            &RecipeContext::AlarmClass(ctx.clone()),
        )
        .unwrap();
        let first_ctx = match &store.load(&id).unwrap().context {
            RecipeContext::AlarmClass(c) => c.clone(),
            _ => panic!(),
        };

        handle_trigger_with_store(&store, Trigger::Fire, &id, &RecipeContext::AlarmClass(ctx))
            .unwrap();
        let second_ctx = match &store.load(&id).unwrap().context {
            RecipeContext::AlarmClass(c) => c.clone(),
            _ => panic!(),
        };

        assert_eq!(first_ctx.label, second_ctx.label);
        assert_eq!(first_ctx.sound_id, second_ctx.sound_id);
        assert_eq!(first_ctx.snooze_count, second_ctx.snooze_count);
        assert_eq!(first_ctx.challenges_solved, second_ctx.challenges_solved);
    }
}

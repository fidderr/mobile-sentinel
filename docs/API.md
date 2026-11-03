# mobile-sentinel API reference

Everything a consumer can call, grouped by layer. Each entry lists the **Cargo
feature** that gates it, the **module path** you call through, the **entry
points**, and any **Android permissions** `build_sentinel` injects when the
feature is enabled.

Conventions used below:

- Unless noted, every `Result<T>` is `Result<T, mobile_sentinel::SentinelError>`.
- On host (non-Android) builds, capability calls return
  `Err(SentinelError::PlatformUnsupported { .. })` or a safe default (`false`,
  `0`, `None`, empty) so your code still compiles and runs in tests.
- Items marked **POLICY-SENSITIVE** are flagged by Google Play review;
  `build_sentinel` prints a warning when they are declared.
- A capability is reached through exactly one feature-gated module — there is no
  raw sink/handle that bypasses the gate.

## Contents

- [Initialization](#initialization)
- [Capabilities](#capabilities)
  - [Capability data types](#capability-data-types)
- [Building blocks](#building-blocks)
  - [StateStore](#statestore-feature-state-store)
  - [Sound Library](#sound-library-feature-sound-library)
  - [Job Guardian](#job-guardian-feature-jobs)
  - [Recurrence Engine](#recurrence-engine-feature-recipes)
  - [Snooze Policy](#snooze-policy-feature-recipes)
- [The FiringSink seam](#the-firingsink-seam-firing-sub-features)
- [The recipe engine](#the-recipe-engine-feature-recipes)
- [AlarmKit](#alarmkit-feature-alarm-kit)
- [Callbacks](#callbacks)
- [Utilities](#utilities)
- [Errors](#errors)
- [Feature dependency graph](#feature-dependency-graph)

---

## Initialization

Always available (no feature required).

```rust
pub fn init(config: InitConfig);

pub struct InitConfig {
    pub logger: bool,                 // logcat logger. Default: true
    pub crash_loop_mitigation: bool,  // SIGABRT → _exit(0). Default: true
    pub clean_process_exit: bool,     // framework exit() → _exit(0), no destructor race. Default: true
    pub panic_log: bool,              // every Rust panic → logcat + files/sentinel/last_panic.txt. Default: true
    pub log_tag: String,              // Default: "MobileSentinel"
    pub log_level: log::LevelFilter,  // Default: Debug (debug builds) / Info (release)
}
```

Call `init(InitConfig::default())` once on app start. No container is returned;
capabilities are reached through their modules.

---

## Capabilities

Each capability is a free-function module gated behind its Cargo feature and
re-exported at the crate root (so you write `mobile_sentinel::camera::…`).

| Feature | Module | Entry points | Permissions / notes |
|---|---|---|---|
| `scanner` | `scanner` | `scan() -> Result<String>`, `is_available() -> bool` | `CAMERA`; bundles ZXing + `SentinelScannerActivity`. Blocks until scan/cancel. |
| `camera` | `camera` | `capture_photo() -> Result<String>`, `is_available() -> bool` | none — delegates to the system camera (`ACTION_IMAGE_CAPTURE`) + `FileProvider`; returns saved JPEG path |
| `haptics` | `haptics` | `vibrate(Duration)`, `vibrate_pattern(&[u64])`, `cancel()`, `has_vibrator() -> bool` | `VIBRATE` |
| `home_widget` | `home_widget` | `set_text(title, detail)`, `can_add() -> bool`, `add() -> bool` | Home-screen widget with two lines the app sets; tapping it opens the app. No permissions, no periodic updates |
| `audio` | `audio` | `play(uri: &str, looping: bool) -> Result<PlaybackHandle>`, `play_as_alarm(uri, looping)` (on the alarm volume, as the ring will sound), `stop(&PlaybackHandle) -> Result<()>`, `set_volume(f32) -> Result<()>` | none — non-firing preview audio |
| `overlay` | `overlay` | `is_granted() -> bool`, `request()` | `SYSTEM_ALERT_WINDOW` |
| `permissions` | `permissions` | `status(&str) -> PermissionState`, `request(&str) -> PermissionState`, `open_app_settings()` | none |
| `sensors` | `sensors` | `start_accelerometer()`, `stop_accelerometer()`, `shake_count() -> i32`, `reset_shake_count()`, `start_step_counter()`, `stop_step_counter()`, `step_count() -> i32` | `ACTIVITY_RECOGNITION` |
| `torch` | `torch` | `on(Option<f32>)`, `off()`, `is_available() -> bool` | none |
| `display` | `display` | `set_brightness(f32)`, `brightness() -> f32`, `set_max_brightness()`, `restore_brightness()`, `keep_screen_on(bool)`, `set_requested_orientation(ScreenOrientation)`, `current_requested_orientation() -> Option<ScreenOrientation>`, `safe_area_insets() -> Option<SafeAreaInsets>` (with how far the keyboard covers the content), `system_dark() -> bool`, `uses_24_hour_clock() -> bool`, `set_light_system_bars(bool)` | none; also supports build-time `screen_orientation = "portrait"` (etc.) in sentinel.toml to set the static default on the main activity |
| `battery` | `battery` | `is_exempt() -> bool`, `request_exemption()`, `open_settings()` | `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` |
| `screen_pin` | `screen_pin` | `pin()`, `unpin()`, `is_pinned() -> bool` | none |
| `foregrounding` | `foregrounding` | `finish_activity()`, `move_to_back()`, `on_back(callback)` | none |
| `media_picker` | `media_picker` | `pick_file(&[&str]) -> Result<String>`, `pick_files(&[&str], multiple: bool) -> Result<Vec<PickedFile>>` | none; `SentinelFilePickerActivity`. Use instead of `<input type="file">` in a WebView. Blocks until the user picked or cancelled. |
| `clipboard` | `clipboard` | `set_text(&str) -> Result<()>`, `get_text() -> Result<Option<String>>`, `has_text() -> bool` | none |
| `share` | `share` | `text(&str, Option<&str>) -> Result<()>`, `url(&str, Option<&str>) -> Result<()>`, `file(&str, &str) -> Result<()>`, `open(path, mime) -> Result<()>`, `open_url(&str) -> Result<()>`, `save_to_downloads(path, name, mime) -> Result<String>` | `WRITE_EXTERNAL_STORAGE`, up to Android 9 |
| `secure_storage` | `secure_storage` | `set(&str, &str) -> Result<()>`, `get(&str) -> Result<Option<String>>`, `delete(&str) -> Result<()>`, `clear() -> Result<()>` | none — private to the app, **not encrypted** |
| `sms` | `sms` | `send(number, message) -> Result<()>`, `is_available() -> bool` | `SEND_SMS` **POLICY-SENSITIVE** |
| `phone` | `phone` | `dial(&str) -> Result<()>` (opens the dialer), `is_in_call() -> bool` | none |
| `network` | `network` | `is_connected() -> bool`, `connection_type() -> ConnectionType` | `ACCESS_NETWORK_STATE` |
| `location` | `location` | `current() -> Result<Coordinate>` (the last known position), `is_enabled() -> bool` | `ACCESS_FINE_LOCATION`, `ACCESS_COARSE_LOCATION` **POLICY-SENSITIVE** |
| `maps` | `maps` | `geocode(&str) -> Result<Coordinate>`, `reverse_geocode(f64, f64) -> Result<String>` | none |
| `biometric` | `biometric` | `is_available() -> bool`, `authenticate(&str) -> Result<()>` (blocks until answered; `Ok` only if recognised), `biometric_type() -> BiometricType` | none |
| `device_admin` | `device_admin` | `is_active() -> bool`, `request()`, `relinquish()` | `BIND_DEVICE_ADMIN` **POLICY-SENSITIVE** |
| `contacts` | `contacts` | `get_all() -> Result<Vec<Contact>>`, `search(&str) -> Result<Vec<Contact>>`, `has_permission() -> bool` | `READ_CONTACTS` **POLICY-SENSITIVE** |
| `calendar` | `calendar` | `get_events(SystemTime, SystemTime) -> Result<Vec<CalendarEvent>>`, `create_event(&CalendarEvent) -> Result<()>`, `delete_event(&str) -> Result<()>` | `READ_CALENDAR`, `WRITE_CALENDAR` **POLICY-SENSITIVE** |
| `dismiss_guard` | `dismiss_guard` | `activate()`, `deactivate()`, `is_active() -> bool` | none; pulls in `screen_pin` |
| `notifications` | `notifications` | `post(id, channel, title, body, importance, full_screen) -> bool`, `update(id, title, body) -> bool`, `cancel(id)` | none (distinct from the firing FGS) |
| `file_system` | `file_system` | `copy_asset(src, dst) -> bool`, `list_assets(path) -> String` (JSON array) | none |
| `accessibility` | `accessibility` | `is_service_enabled() -> bool`, `open_settings() -> Result<(), String>` | `SentinelAccessibilityService` **POLICY-SENSITIVE**; pulls in `kiosk` |
| `recorder` | `recorder` | `has_permission() -> bool`, `request_permission()`, `start(dir) -> Result<()>`, `level() -> f32`, `is_recording() -> bool`, `stop() -> Result<Recording>`, `cancel()` | `RECORD_AUDIO` |
| `background_service` | `background_service` | `set_texts(...)`, `start() -> bool`, `stop()`, `is_enabled() -> bool`, `post(tag, title, body, open, kind)`, `cancel(tag)`, `cancel_all()`, `take_open_request() -> Option<String>`, `is_foreground() -> bool`, `move_to_back() -> bool`, `exit_app() -> bool`, `warm_up()` | `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_REMOTE_MESSAGING`, `POST_NOTIFICATIONS`, `RECEIVE_BOOT_COMPLETED`, `USE_FULL_SCREEN_INTENT`. Keeps a messenger's connection with the UI closed |
| `webview_media` | none | none: declares what `getUserMedia` needs inside the app's WebView | `CAMERA`, `RECORD_AUDIO`, `MODIFY_AUDIO_SETTINGS` |

### Examples

**Audio preview** (a settings-screen tone preview):

```rust
// requires `audio`
let handle = mobile_sentinel::audio::play("/path/to/sound.mp3", /* looping */ true)?;
mobile_sentinel::audio::set_volume(0.7)?;
mobile_sentinel::audio::stop(&handle)?;

// The handle id is a plain u64 you can stash across an FFI / UI boundary:
let id = handle.id();
let same = mobile_sentinel::PlaybackHandle::from_id(id);
```

**Display / orientation** (app-wide portrait lock via build, per-screen override on settings):

```rust
// 1. In sentinel.toml (or via prepare fn) set a default:
//    [android]
//    screen_orientation = "portrait"

// 2. At runtime (requires `display` feature) — only allow rotation while
//    the settings screen is visible:
use mobile_sentinel::display::{set_requested_orientation, ScreenOrientation};

set_requested_orientation(ScreenOrientation::Unspecified); // or FullSensor

// ... later, on unmount / leaving settings:
set_requested_orientation(ScreenOrientation::Portrait);
```

**Scanner** (blocks until the user scans or cancels):

```rust
// requires `scanner`
match mobile_sentinel::scanner::scan() {
    Ok(value) => println!("scanned: {value}"),
    Err(_)    => println!("cancelled / unavailable"),
}
```

**Permissions + overlay** (typical startup prompts):

```rust
// requires `permissions` + `overlay`
use mobile_sentinel::{permissions, overlay, PermissionState};

if permissions::request("android.permission.POST_NOTIFICATIONS") != PermissionState::Granted {
    permissions::open_app_settings(); // deep-link so the user can re-grant
}
if !overlay::is_granted() {
    overlay::request();
}
```

**Sensors** (shake + step counters):

```rust
// requires `sensors`
use mobile_sentinel::sensors::{start_accelerometer, shake_count, reset_shake_count,
                               stop_accelerometer, start_step_counter, step_count};

start_accelerometer();
let shakes = shake_count(); // accumulating count since start
reset_shake_count();
stop_accelerometer();

start_step_counter();
let steps = step_count();   // Sensor.TYPE_STEP_COUNTER where supported
```

The accelerometer thresholds the motion-only magnitude (gravity subtracted) with
a short cooldown, so one gesture counts once.

### Capability data types

Structured values capability functions accept and return, re-exported at the
crate root:

```rust
pub enum PermissionState { Granted, Denied, NotDetermined }
pub enum BiometricType   { Face, Fingerprint, None }
pub enum ConnectionType  { Wifi, Cellular, None }

pub struct Coordinate { pub latitude: f64, pub longitude: f64 }

pub struct Contact {
    pub id: String,
    pub display_name: String,
    pub phone_numbers: Vec<String>,
    pub email_addresses: Vec<String>,
}

pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub start_time: std::time::SystemTime,
    pub end_time: std::time::SystemTime,
    pub location: Option<String>,
}

// Opaque audio playback handle (from `audio::play`).
pub struct PlaybackHandle(/* private u64 */);
impl PlaybackHandle {
    pub fn id(&self) -> u64;
    pub fn from_id(id: u64) -> Self;
}

// Opaque per-instance identifier (StateStore / ContextStore key).
pub struct InstanceId(pub String);
impl InstanceId {
    pub fn new(s: impl Into<String>) -> Self;
    pub fn as_str(&self) -> &str;
}
```

---

## Building blocks

Reusable Rust primitives with value on their own. Each is its own feature;
AlarmKit composes them, but you can use them directly.

### StateStore (feature `state-store`)

An atomic, crash-safe, per-id JSON store generic over your own record type.

| Property | Guarantee |
|---|---|
| Layout | one JSON file per instance: `<root>/<id>.json` |
| Atomicity | write `<id>.json.tmp`, then `rename` (POSIX / Win32 atomic). Readers see pre- or post-write, never partial. |
| Concurrency | per-id `Mutex` serializes writers; reads are lock-free |
| Revision | every write sets `revision = previous + 1` from disk state — the caller-supplied revision is ignored |
| Resilience | unparseable files are logged with `warn!` and skipped, never propagated |

```rust
use mobile_sentinel::{StateStore, Stateful, StateRevision, InstanceId, app_files_dir};
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
struct Note { id: InstanceId, revision: StateRevision, text: String }

impl Stateful for Note {
    fn instance_id(&self) -> &InstanceId { &self.id }
    fn revision(&self) -> StateRevision { self.revision }
    fn with_revision(mut self, r: StateRevision) -> Self { self.revision = r; self }
}

let store: StateStore<Note> = StateStore::new(app_files_dir().join("notes"));
let written = store.write(Note {
    id: InstanceId::new("n1"), revision: StateRevision::default(), text: "hi".into(),
})?;                                  // written.revision() == 1
let loaded = store.load(&InstanceId::new("n1"))?;
let all    = store.enumerate_all()?;
let exists = store.exists(&InstanceId::new("n1"));
let count  = store.count()?;
store.delete(&InstanceId::new("n1"))?;
```

Methods: `new`, `root`, `load`, `exists`, `write`, `delete`, `enumerate_all`,
`count`. Error type: `StateStoreError` (variants `NotFound`, `Io`, `Serde`,
`IdMismatch`, `LockPoisoned`).

The recipe layer's `ContextStore` is exactly `StateStore<ContextRecord>` — the
same machinery, fixed to the recipe `ContextRecord` payload.

### Sound Library (feature `sound-library`)

Bundled + custom + system-default sound resolution.

```rust
use mobile_sentinel::{SoundLibrary, SoundId, AndroidSoundBackend, app_files_dir};

let library = SoundLibrary::new(
    /* bundled_dir */ app_files_dir().join("sounds/default"),
    /* custom_dir  */ app_files_dir().join("sounds/custom"),
    backend, // implements SoundBackend::system_default_uri()
);

let bundled: Vec<SoundEntry> = library.enumerate_bundled()?;
let custom:  Vec<SoundEntry> = library.enumerate_custom()?;

let uri = library.resolve(&SoundId::Bundled("happy".into()));
let uri = library.resolve(&SoundId::SystemDefault);
let uri = library.resolve(&SoundId::Silent);   // "silent://"

let id: SoundId = library.import(std::path::Path::new("/sdcard/Music/wake.mp3"))?;
library.delete_custom("<token>")?;
```

`SoundId`:

```rust
pub enum SoundId {
    Bundled(String),  // file stem under bundled_dir, e.g. "happy"
    Custom(String),   // opaque UUID token under custom_dir
    SystemDefault,    // resolved via SoundBackend::system_default_uri()
    Silent,           // "silent://" — vibrate-only
}
impl SoundId { pub fn is_silent(&self) -> bool; }
```

`SoundEntry { id: SoundId, label: String, path: PathBuf }`. Allowed extensions:
`mp3`, `m4a`, `ogg`, `wav`. Empty files and unsupported extensions are rejected.
If a referenced file is missing, `resolve` falls back to the system-default URI
and logs a `warn!` — the consumer never special-cases "the old custom sound was
deleted." On Android, construct the backend with
`mobile_sentinel::AndroidSoundBackend`. Methods: `new`, `bundled_dir`,
`custom_dir`, `enumerate_bundled`, `enumerate_custom`, `resolve`, `import`,
`delete_custom`. Error type: `SoundError`.

### Job Guardian (feature `jobs`)

Generic, polling-based job persistence for the `:sentinel` process. The guardian
knows nothing about alarms — only "some jobs need MAIN alive."

```rust
use mobile_sentinel::{register_job, activate_job, deactivate_job, complete_job,
                      get_active_jobs, get_job, remove_job, jobs_dir, JobConfig};

let job = register_job(
    "alarm-42",
    serde_json::json!({ "instance_id": "alarm-42" }), // opaque payload
    JobConfig::default(),
)?;

activate_job("alarm-42")?;     // pending → active (the OS wake does this)
deactivate_job("alarm-42")?;   // active → pending (pause; file survives)
complete_job("alarm-42")?;     // done — removes the file (or marks Completed)
```

```rust
pub struct JobConfig {
    pub poll_interval_ms: u64,         // default 500
    pub start_main_delay_ms: u64,      // default 0
    pub heads_up_delay_ms: u64,        // default 200
    pub auto_remove_on_complete: bool, // default true
    pub ui_liveness_timeout_ms: u64,   // 0 = off (default). Restart MAIN if its UI sends no `jobs::ui_heartbeat()` for this long while the main activity is on screen
    pub ui_liveness_backoff_cap: u32,  // default 4: each kill doubles the allowed silence, up to this multiple
    pub min_launch_interval_ms: u64,   // default 3000: never launch MAIN more often than this
    pub ui_failures_before_backup: u32, // 0 = off (default). After this many UI restarts the native backup screen takes over
}

// The UI proves it is alive with `jobs::ui_heartbeat()`, called only after a real
// round trip through the UI worked. The `:sentinel` guardian restarts MAIN when the
// heartbeat stops (policy above comes from this config; the guardian only enforces it).
// State written before the current boot is ignored by the guardian and the kiosk
// lock, and `AlarmKit::on_startup` ends such rings (see `StartupReport`).

pub enum JobStatus { Pending, Active, Completed }

pub struct Job {
    pub id: String,
    pub status: JobStatus,
    pub payload: serde_json::Value,   // opaque to sentinel; consumer-interpreted
    pub config: JobConfig,
}
```

Full API: `register_job`, `activate_job`, `deactivate_job`, `complete_job`,
`get_active_jobs`, `get_job`, `remove_job`, `jobs_dir`, `active_since_ms`, `ring_end`,
`ui_heartbeat`. Error type: `JobGuardianError` (`Io`, `Parse`, `NotFound`).

`deactivate` vs `complete`: `complete_job` means *done* (file removed, guardian
never acts again); `deactivate_job` means *pause until the next trigger* (file
stays `Pending` so `activate_job` can revive it — this is the snooze lifecycle).

The `:sentinel` Kotlin guardian polls `<files>/sentinel/jobs/*.json` at
`poll_interval_ms`. Per active job: if MAIN is dead it starts MAIN (after
`start_main_delay_ms`); if MAIN is alive it waits `heads_up_delay_ms` then sends
a heads-up broadcast. Wire the MAIN-side landing pad with
`callbacks::on_job_heads_up(...)` (AlarmKit does this for you).

### Recurrence Engine (feature `recipes`)

DST-correct next-fire computation. Pure, deterministic, property-tested.

```rust
use mobile_sentinel::{next_fire, Schedule, WeekdaySet};
use chrono::{NaiveTime, Weekday, Utc};
use chrono_tz::Tz;

let schedule = Schedule::Weekdays {
    days: WeekdaySet::from_weekdays([Weekday::Mon, Weekday::Wed, Weekday::Fri]),
    time: NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
};
let zone: Tz = "Europe/Amsterdam".parse().unwrap();
let next = next_fire(&schedule, &zone, Utc::now())?;
```

`Schedule` variants:

| Variant | Meaning |
|---|---|
| `OneTime { date: NaiveDate, time: NaiveTime }` | Single fire at a local date + time-of-day. |
| `Weekdays { days: WeekdaySet, time: NaiveTime }` | Recurring on selected weekdays. |
| `Monthly { day_of_month: u8, time: NaiveTime }` | `0` = last day; values past month length clamp down. |
| `Cron { expression: String }` | Reserved — returns `RecurrenceError::CronUnsupported`. |

`WeekdaySet` (bit 0 = Mon … bit 6 = Sun): constants `NONE`, `ALL`, `WEEKDAYS`
(Mon–Fri), `WEEKENDS` (Sat+Sun); methods `from_weekdays([...])`, `contains`,
`insert`, `iter`, `is_empty`. The engine resolves DST: a local time in a
spring-forward gap resolves to the earliest valid post-gap instant; a fall-back
overlap picks the first occurrence. Errors: `NoFireTime` (empty set, or a
`OneTime` in the past), `CronUnsupported`, `InternalDateFailure`.

### Snooze Policy (feature `recipes`)

```rust
use mobile_sentinel::{SnoozePolicy, EscalationPolicy};

let constant = SnoozePolicy::constant(/* max_count */ 3, /* interval_minutes */ 5);

let linear = SnoozePolicy {
    max_count: 4, interval_minutes: 5,
    escalation: Some(EscalationPolicy::Linear { step_minutes: 2 }),
}; // 5, 7, 9, 11

let exponential = SnoozePolicy {
    max_count: 4, interval_minutes: 5,
    escalation: Some(EscalationPolicy::Exponential { factor: 2.0 }),
}; // 5, 10, 20, 40

let custom = SnoozePolicy {
    max_count: 5, interval_minutes: 5,
    escalation: Some(EscalationPolicy::Custom { intervals: vec![1, 3, 7, 15] }),
}; // 1, 3, 7, 15, then 5 (fallback past the list)
```

Methods: `can_snooze(current_count) -> bool`,
`interval_for(snooze_index) -> Option<u32>`. `max_count == 0` disables snooze
entirely. NaN / negative / overflowing factors fall back to the base interval or
saturate.

---

## The FiringSink seam (firing sub-features)

The minimal, mockable platform interface the recipe engine drives. Compiles when
any firing sub-feature is enabled (`wake-lock`, `firing-audio`,
`firing-vibration`, `foreground-service`, `exact-alarm`, `kiosk`,
`full-screen-intent`; `firing` is the convenience bundle of all seven).

```rust
pub trait FiringSink: Send + Sync {
    fn start_firing(&self, req: &FireRequest) -> bool;
    fn stop_firing(&self, instance_id: &str);
    fn pause_audio(&self);
    fn schedule_exact_alarm(&self, req: &ExactAlarmRequest) -> bool;
    fn cancel_exact_alarm(&self, alarm_id: &str);
    fn system_default_sound_uri(&self) -> String;
}
```

Implementations: `AndroidFiringSink` (JNI; `mobile_sentinel::AndroidFiringSink`,
Android only) and `MockFiringSink` (records every call, for tests). `FireRequest`
carries the channel/notification config, resolved `sound_uri`, vibration flag +
waveform, and the kiosk / full-screen knobs; `ExactAlarmRequest` carries
`{ alarm_id, target_unix_ms, metadata_json }`. See the type definitions in
`firing.rs` for the full field list.

> `pause_audio` only quiets sound. Re-engaging is always `start_firing` — there
> is no `resume_audio`.

---

## The recipe engine (feature `recipes`)

The reusable state-machine framework prebuilt recipes are built on. You can
implement your own `Recipe` and drive it without enabling `alarm-kit`.

### Trigger

The typed catalogue of every state-transition entry point. Serializable;
cross-process dispatch round-trips through `as_str` / `parse_tag`.

```rust
pub enum Trigger {
    Fire,      // scheduled time reached or snooze elapsed
    Snooze,    // user-initiated snooze
    Dismiss,   // user-initiated dismiss
    Solve,     // challenge solved — unlocks dismiss
    Edit,      // instance edited while scheduled
    Schedule,  // arm a new OS-level wake
    Pause,     // quiet firing audio
    Resume,    // re-engage the firing surface (full start_firing)
}
impl Trigger {
    pub fn as_str(self) -> &'static str;
    pub fn parse_tag(s: &str) -> Option<Self>;
    pub const ALL: &'static [Trigger];
}
```

### The Recipe trait

```rust
pub trait Recipe: Send + Sync + 'static {
    fn recipe_type(&self) -> &'static str;                       // stable tag = Context variant
    fn required_permissions(&self) -> &'static [RecipePermission] { &[] }
    fn handle_trigger(
        &self,
        trigger: Trigger,
        instance_id: &InstanceId,
        context: &RecipeContext,   // freshly loaded from disk by the dispatcher
    ) -> Result<(), RecipeError>;
}

pub struct RecipePermission {
    pub name: &'static str,        // e.g. "android.permission.POST_NOTIFICATIONS"
    pub rationale: &'static str,
    pub required: bool,
    pub min_api: Option<u32>,
}
```

### Dispatch + registry

```rust
use mobile_sentinel::{register_recipe, dispatch_trigger, recipe_registry,
                      Trigger, InstanceId, ContextStore};

register_recipe(MyRecipe::new())?;             // one-shot; duplicate type is rejected
dispatch_trigger(&store, Trigger::Fire, &id)?; // loads Context, routes to the recipe
```

`dispatch_trigger` is the single entry point: it (1) loads the Context from disk
(`RecipeError::ContextNotFound` if absent), (2) resolves the recipe by its
`recipe_type` tag (`RecipeError::RecipeNotRegistered` if unknown), then (3) calls
`handle_trigger` with the freshly-loaded Context. Reloading on every trigger is
the **LoadContext-on-every-trigger** invariant that makes a process restart at
any point safe. `register_recipe` returns `RegistrationError::DuplicateRecipe` on
a repeat type. `recipe_registry()` exposes `get(type)` and `registered_types()`.

### Context schema + ContextStore

Instance state persists as a `RecipeContext` inside a `ContextRecord` envelope
`{ id, revision, state, context }`, stored in `ContextStore` (=
`StateStore<ContextRecord>`).

```rust
pub enum SessionState {
    Idle,
    Scheduled { next_fire_unix_ms: i64 },
    Firing    { fired_at_unix_ms: i64 },
    Snoozed   { snooze_count: u32, next_fire_unix_ms: i64 },
    Dismissed { dismissed_at_unix_ms: i64 },
}
impl SessionState { pub fn tag(&self) -> &'static str; }

pub enum RecipeContext {
    AlarmClass(AlarmClassContext),
    Custom { payload_json: String },  // escape hatch for consumer recipes
}
impl RecipeContext { pub fn recipe_type(&self) -> &'static str; }

pub struct AlarmClassContext {
    pub label: String,
    pub schedule: ScheduleSpec,
    pub time_zone: String,                       // IANA, e.g. "Europe/Amsterdam"
    pub sound_id: SoundIdSpec,
    pub snooze_policy: SnoozePolicy,
    pub challenges: Vec<ChallengeSpec>,          // opaque, consumer-interpreted
    pub vibration_enabled: bool,
    pub vibration_pattern: Option<Vec<i64>>,     // None = use AlarmClassConfig default
    pub kiosk_mode: bool,
    pub bypass_dnd: bool,
    pub snooze_count: u32,                        // runtime state
    pub challenges_solved: bool,                  // runtime state
}
```

`ScheduleSpec` (the persisted form; `Schedule` is the in-memory working type):

```rust
pub enum ScheduleSpec {
    OneTime  { date: String /* YYYY-MM-DD */, hour: u8, minute: u8 },
    Weekdays { days_mask: u8 /* bit0=Mon … bit6=Sun */, hour: u8, minute: u8 },
    Monthly  { day_of_month: u8 /* 0 = last */, hour: u8, minute: u8 },
    Cron     { expression: String },
}

pub enum SoundIdSpec { Bundled(String), Custom(String), SystemDefault, Silent }

pub struct ChallengeSpec {
    pub challenge_type: String,    // consumer-defined
    pub difficulty: u8,            // consumer-interpreted
    pub config: serde_json::Value,
}
```

`ContextRecord::new(id, context)` starts at `Revision::INITIAL` in
`SessionState::Idle`. Error type for the recipe layer: `RecipeError`
(`InvalidContext`, `ContextNotFound`, `RecipeNotRegistered`, `Recurrence`,
`Store`, `IllegalTransition`, `Other`).

---

## AlarmKit (feature `alarm-kit`)

A prebuilt recipe wrapper that composes the engine + firing + sound + jobs into a
complete alarm backend. It owns no engine logic — it drives the `AlarmClass`
recipe through `ContextStore` + the installed `FiringSink`.

### Install

```rust
use mobile_sentinel::{AlarmKit, AlarmKitConfig};

pub struct AlarmKitConfig {
    pub store: Arc<ContextStore>,
    pub sink: Option<Arc<dyn FiringSink>>,                  // None on host
    pub sound_resolver: Option<Arc<dyn SoundResolver>>,     // a SoundLibrary, or None
    pub alarm_class_config: AlarmClassConfig,
}

// What the platform rings with: the Android sink and sound backend, and the context
// store in the app's files. All an app normally needs.
let kit = AlarmKit::install_on_platform(bundled_sounds, custom_sounds, alarm_class_config);

let kit = AlarmKit::install(config);          // the same with your own parts
```

`AlarmClassConfig` — the app-specific firing wiring AlarmKit hands the sink:

```rust
pub struct AlarmClassConfig {
    pub channel_id: String,
    pub channel_name: String,
    pub firing_body: String,
    pub importance: i32,                 // 5 = IMPORTANCE_MAX
    pub audio_usage: String,             // e.g. "alarm"
    pub audio_content_type: String,      // e.g. "sonification"
    pub activity_fqcn: Option<String>,   // full-screen-intent + kiosk relaunch target
    pub kiosk_debounce_ms: u32,
    pub kiosk_block_home: bool,
    pub kiosk_block_back: bool,
    pub kiosk_block_recents: bool,
    pub kiosk_block_volume: bool,
    pub kiosk_hide_status_bar: bool,
    pub kiosk_hide_nav_bar: bool,
    pub firing_full_screen: bool,
    pub vibration_pattern: Vec<i64>,     // app-default waveform (alternating wait/vibrate ms)
    pub min_volume_percent: u8,          // raise the alarm stream to at least this % while ringing, restore after (0 = leave alone)
    pub late_ring_window_ms: i64,        // found this late after a restart it still rings; later it is a miss
    pub fallback_sound_id: Option<SoundIdSpec>, // tried when the alarm's own sound cannot play
}
```

Alarms have to be armed again with no app open: after a boot, an app update, a clock or
time zone change. The app says what runs then, once, anywhere in its crate:

```rust
fn on_wake() { /* set up, and arm what has to ring. No Activity here. */ }
mobile_sentinel::wake_hook!(on_wake);
```

### AlarmSpec

The consumer-facing alarm description (maps 1:1 to `AlarmClassContext` minus the
runtime fields). `AlarmSpec::default()` is a sensible 07:00 one-time alarm.

```rust
pub struct AlarmSpec {
    pub label: String,
    pub schedule: ScheduleSpec,
    pub time_zone: String,
    pub sound_id: SoundIdSpec,
    pub snooze_policy: SnoozePolicy,
    pub challenges: Vec<ChallengeSpec>,
    pub vibration_enabled: bool,
    pub vibration_pattern: Option<Vec<i64>>,  // None = inherit AlarmClassConfig default
    pub kiosk_mode: bool,
    pub bypass_dnd: bool,
    pub skip_until_unix_ms: Option<i64>,      // skip every occurrence at or before this instant (one skipped occurrence)
    pub fade_in_secs: u16,                    // sound fade-in on the first ring (0 = off); not applied on resume
    pub ring_limit_secs: Option<u32>,         // stop by itself after this long with nobody doing anything; None rings until dealt with
    pub min_volume_percent: Option<u8>,       // None = AlarmClassConfig's
    pub sound_delay_secs: u16,                // vibrate first, the sound this much later
}
impl AlarmSpec {
    pub fn from_context(ctx: &AlarmClassContext) -> Self;
}
```

### Lifecycle methods

```rust
kit.create(spec) -> Result<InstanceId, AlarmKitError>            // new alarm + arm exact alarm
kit.create_with_id(id, spec) -> Result<(), AlarmKitError>        // choose the id (migrations)
kit.update(&id, spec) -> Result<(), AlarmKitError>               // re-arm with new schedule
kit.delete(&id) -> Result<(), AlarmKitError>                     // cancel + remove
kit.snooze(&id) -> Result<(), AlarmKitError>                     // → Snoozed, re-arm, deactivate job
kit.dismiss(&id) -> Result<(), AlarmKitError>                    // → Dismissed, re-arm next occurrence
kit.mark_solved(&id) -> Result<(), AlarmKitError>                // challenge gate passed
kit.pause(&id) -> Result<(), AlarmKitError>                      // quiet audio (enter a challenge screen)
kit.resume(&id) -> Result<(), AlarmKitError>                     // full re-engage (alias: reengage)
kit.reengage(&id) -> Result<(), AlarmKitError>                   // re-engage firing surface (MAIN restart)
kit.handle_job_heads_up(job_id: &str)                            // sentinel heads-up landing pad
kit.on_startup(&[(String, AlarmSpec)])                           // end rings a reboot interrupted, rearm (a snooze in progress is kept), process jobs, reengage firing -> StartupReport { ended_by_reboot, missed, finished, ring_ends }
kit.on_wake(&[(String, AlarmSpec)])                              // the same with no screen: from the wake hook
kit.refresh_ring_plans()                                         // write the ring plans again, after what they depend on changed (minimum volume, texts)
kit.note_activity()                                              // the user did something: the ring limit starts over
kit.note_ui_up(&id)                                              // the alarm's screen is up, for the timing log
kit.on_ring_end(hook: fn())                                      // called when the platform ended a ring while the app runs
kit.take_ring_ends() -> Vec<(InstanceId, RingEnd)>               // rings that ended without the app: limit reached, solved on the backup screen
kit.set_reminder(&id, at_ms, title, body, channel_name, lasts_ms) // a quiet notification before an alarm
kit.clear_reminder(&id)
```

### Inspection

```rust
kit.list() -> Result<Vec<(InstanceId, AlarmSpec)>, AlarmKitError>
kit.get(&id) -> Result<AlarmSpec, AlarmKitError>
kit.next_fire(&id) -> Result<Option<i64>, AlarmKitError>         // unix ms, Scheduled/Snoozed only
kit.snoozed_until(&id) -> Option<i64>                            // unix ms, if it is snoozed right now
kit.current_session() -> Result<Option<AlarmKitSession>, AlarmKitError>
kit.session(&id) -> Option<AlarmKitSession>                      // of this alarm, if it is ringing
kit.timing_log() -> String                                       // how fast rings were: `<unix ms> <event> <alarm>` per line
kit.required_permissions() -> Vec<&'static str>
kit.store() -> &Arc<ContextStore>
AlarmKit::detect_timezone() -> String                           // device tz, falls back to TZ / "UTC"
```

```rust
pub struct AlarmKitSession {
    pub instance_id: InstanceId,
    pub fired_at_unix_ms: i64,      // when this ring started; a snooze ending starts a new one
    pub first_fired_unix_ms: i64,   // when the first ring of this session started
    pub spec: AlarmSpec,
    pub snooze_count: u32,
    pub challenges_solved: bool,
}
```

Error type: `AlarmKitError` (`NotFound`, `Store`, `Recipe`).

### What the phone allows

`mobile_sentinel::health`, with `alarm-kit`. What stands in the way of an alarm ringing on
time, and the way to the setting for it.

```rust
let report = health::report(channel_id);          // HealthReport
report.is_ok(Check::ExactAlarms);
report.problems();                                // Vec<Check>, most serious first
health::open_settings(Check::Overlay, channel_id); // false if there is no such page
```

`Check`: `ExactAlarms`, `Notifications`, `FullScreen`, `Overlay`, `BatteryUnrestricted`,
`BackgroundAllowed`, `KeptWhenUnused`, `ChannelImportance`, `NotTotalSilence`, `BootSignal`.

### Custom sound resolution

```rust
pub trait SoundResolver: Send + Sync {
    fn resolve(&self, id: &SoundIdSpec) -> String;  // → playable URI
}
```

`SoundLibrary<B>` implements `SoundResolver` automatically when `sound-library`
is enabled (the recipe layer adapts `SoundIdSpec` ↔ `SoundId` at its boundary).

---

## Callbacks

Cross-platform registration for Android system events (no-ops on host). Always
available via `mobile_sentinel::callbacks`.

```rust
mobile_sentinel::callbacks::on_boot_completed(|| {
    // re-arm alarms after reboot
});

mobile_sentinel::callbacks::on_job_heads_up(|job_id: String| {
    // the :sentinel guardian says "this job needs MAIN"; AlarmKit wires a
    // default handler for you, but you can override it after install()
});
```

---

## Utilities

Always available (no feature required).

```rust
// App's files directory. On Android the one the Kotlin side uses, device-encrypted when
// `direct_boot` is set, also with no Activity; elsewhere ".".
pub fn app_files_dir() -> std::path::PathBuf;

// The device's language as a two-letter code ("nl"), empty when unknown.
pub fn device_language() -> String;

// Extract bundled APK assets to the filesystem (idempotent — skips existing).
pub struct AssetExtractor { /* … */ }
impl AssetExtractor {
    pub fn new(target_base: PathBuf) -> Self;
    pub fn extract(&self, asset_subdir: &str, target_subdir: &str) -> usize;       // Android: APK assets
    pub fn extract_from_dir(&self, source_dir: &Path, target_subdir: &str) -> usize; // desktop/testing
}
```

---

## Errors

```rust
pub enum Platform { Android, Unsupported }

pub enum ErrorCode {
    ResourceBusy, InvalidArgument, Timeout, Internal, IoError, Unsupported,
}

pub enum SentinelError {
    PlatformUnsupported {
        platform: Platform,
        feature: String,
        fallback_suggestion: Option<String>,
    },
    RuntimeError { code: ErrorCode, message: String },
}
impl SentinelError {
    // Standard host-fallback constructor used by every capability module.
    pub fn unavailable(feature: &str) -> Self;
}
```

Layer-specific error types (kept shallow): `StateStoreError` / `StoreError`,
`SoundError`, `JobGuardianError`, `RecurrenceError`, `RecipeError`,
`RegistrationError`, `AlarmKitError`.

---

## Feature dependency graph

```text
alarm-kit = ["recipes", "firing", "sound-library", "jobs"]
recipes   = ["state-store"]
firing    = ["wake-lock", "firing-audio", "firing-vibration",
             "foreground-service", "exact-alarm", "kiosk", "full-screen-intent"]

wake-lock, firing-audio, firing-vibration = ["foreground-service"]
foreground-service, exact-alarm, kiosk, full-screen-intent, jobs = ["ring"]

dismiss_guard = ["screen_pin"]
accessibility = ["kiosk"]
```

`ring` is what a ring is made of; it comes with what needs it. All other features (the leaf
capabilities and the building blocks `state-store` and `sound-library`) have no
dependencies: enable exactly what you call.

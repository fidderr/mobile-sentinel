# mobile-sentinel

A Rust-first Android SDK. You write your app's logic in Rust; Kotlin shrinks to
thin JNI shims that do exactly what Rust tells them. One language owns the
lifecycle, the state, the recovery — and one place explains what happened when
something breaks.

mobile-sentinel is **generic**. It does not assume you are building an alarm
clock, a timer, a kiosk, or a meditation app. It hands you Android's
capabilities — audio, alarms, foreground services, kiosk lock-task, camera and
scanners, sensors, pickers, notifications, permissions, biometrics, location,
SMS, and more — as **feature-gated Rust modules**. On top of those it ships
reusable building blocks (a durable state store, a cross-process job guardian, a
DST-correct recurrence engine, a snooze policy, a sound library), a small
**recipe engine** (a `Trigger`/`Recipe` state-machine framework), and
**AlarmKit**, a prebuilt recipe that gives you a complete alarm backend out of
the box.

The rule that holds the whole design together: **a feature you do not enable
ships nothing.** No Rust in your `.so`, no Android manifest permission or
component, no Kotlin Gradle module. Calling a capability whose feature is off is
a compile error, not a silent runtime no-op.

> **New here?** Read [Mental model](#mental-model) then
> [Quick start](#quick-start). For the exhaustive list of everything a consumer
> can call — every capability, building block, recipe, and type — see
> **[docs/API.md](docs/API.md)**.

---

## Contents

- [Why it exists](#why-it-exists)
- [Mental model](#mental-model)
- [How feature gating works](#how-feature-gating-works)
- [Quick start](#quick-start)
- [Initialization](#initialization)
- [The FiringSink seam](#the-firingsink-seam)
- [The recipe engine and AlarmKit](#the-recipe-engine-and-alarmkit)
- [Architecture: two processes, one filesystem](#architecture-two-processes-one-filesystem)
- [Building the APK (build_sentinel)](#building-the-apk-build_sentinel)
- [Platform support](#platform-support)
- [Errors](#errors)
- [Testing](#testing)
- [Adding a capability](#adding-a-capability)
- [API reference](#api-reference)

---

## Why it exists

Android apps that do anything past drawing a screen usually end up half-Kotlin,
half-business-logic, with the platform glue tangled through both. mobile-sentinel
inverts that split:

- **Rust decides everything.** Scheduling, firing, snooze, dismiss, kiosk,
  foreground service, persistence, recovery — all Rust.
- **Kotlin executes.** "Play this URI." "Schedule this exact alarm." "Post this
  notification." "Block HOME." Nothing more.
- **Cross-process by design.** A separate `:sentinel` process watches files and
  resurrects the main process if the OS kills it. It is generic — it knows
  nothing about your app.
- **Portable seam.** The only platform interface the engine drives is a narrow
  6-method trait (`FiringSink`). An iOS backend can slot in behind it without
  touching consumer code.

## Mental model

Three kinds of thing live in this crate. Keeping them straight is the whole
game:

| Thing | What it is | Examples |
|---|---|---|
| **Capability** | A thin, feature-gated Rust module that calls one area of the Android framework over JNI. The *only* door to that platform area. | `scanner::scan()`, `haptics::vibrate()`, `location::current()` |
| **Building block** | A reusable, mostly-pure Rust primitive with value on its own. | `StateStore`, `SoundLibrary`, the Job Guardian, recurrence, snooze |
| **Recipe** | A prebuilt behavior bundle that *composes* building blocks + capabilities so you don't wire them by hand. Owns no platform logic itself. | `AlarmKit` |

The layering is strictly one-way, top to bottom:

```
   AlarmKit                      prebuilt recipe          feature "alarm-kit"
      │ composes
      ▼
   recipe engine                 Trigger, Recipe,         feature "recipes"
      │                          dispatch, registry,
      │                          ContextStore, recurrence, snooze
      ▼
   building blocks   +   capabilities                     one feature each
   (state-store,         (camera, audio, sensors, …       reached via
    sound-library,        reached via mobile_sentinel::<cap>)
    jobs)
      │
      ▼
   FiringSink seam               6 methods the engine drives
      │
      ▼
   Kotlin (:sentinel-core + per-capability Gradle modules) over JNI
```

You can stop at any layer: use one capability and nothing else, use the state
store on its own, write your own `Recipe` against the engine, or just take
AlarmKit and get a finished alarm backend.

**The floor (the "husk").** With *no* features you get only the
`:sentinel-core` kernel — a context holder, an activity tracker, and the JNI
bridge. Nothing is callable. `apps/husk` exists only to prove this floor stays
empty.

## How feature gating works

Capabilities are **Cargo features** on the mobile-sentinel dependency. That is
the single, compile-enforced source of truth.

```toml
[dependencies]
mobile-sentinel = { path = "../crates/mobile-sentinel", features = [
    "alarm-kit", "scanner", "haptics", "audio",
] }
```

That one declaration drives three things from the same list:

1. **Rust** — only the enabled capability modules compile. Calling
   `mobile_sentinel::scanner::scan()` without `scanner` is a compile error at
   the call site. A disabled feature contributes zero bytes to the native `.so`.
2. **Android manifest** — `build_sentinel` injects only the permissions and
   components the enabled features need. A capability you didn't enable never
   appears in the shipped APK's merged manifest, so Google Play's static scan
   never sees it.
3. **Kotlin / Gradle** — each capability is its own Gradle module
   (`android/caps/<id>`). `build_sentinel` wires `:sentinel-core` plus only the
   enabled modules, so a disabled capability's Kotlin and its external Gradle
   dependencies are never compiled.

`build.rs` records the enabled feature set to `OUT_DIR/enabled_capabilities.txt`
so `build_sentinel` derives the trimming from the exact list the Rust compiler
used.

Two kinds of feature:

- **Capability features** (`scanner`, `camera`, `haptics`, `audio`, `sensors`,
  `sms`, …) expose a Rust API and contribute manifest/Kotlin pieces.
- **Building-block / composite features** (`state-store`, `sound-library`,
  `jobs`, `recipes`, `alarm-kit`, and the firing sub-features) are pure-Rust or
  composition features. `alarm-kit` pulls in everything an alarm needs:

  ```text
  alarm-kit = ["recipes", "firing", "sound-library", "jobs"]
  recipes   = ["state-store"]
  firing    = ["wake-lock", "firing-audio", "firing-vibration",
               "foreground-service", "exact-alarm", "kiosk",
               "full-screen-intent"]
  ```

The capability id is the spine. The Cargo feature name is the **kebab** form of
the id and the `CARGO_FEATURE_*` env var is the **upper** form
(`full_screen_intent` ↔ feature `full-screen-intent` ↔ `FULL_SCREEN_INTENT`).
The mapping is mechanical, never a hand-written table.

## Quick start

### 1. Add the dependency

Declare exactly the capabilities you call — nothing more.

```toml
[dependencies]
mobile-sentinel = { path = "../crates/mobile-sentinel", features = [
    "alarm-kit", "scanner", "haptics", "audio", "overlay", "permissions",
    "sensors", "foregrounding", "media_picker",
] }
```

### 2. Initialize on app start

```rust
use mobile_sentinel::{init, InitConfig};

init(InitConfig::default());
```

`init` configures the logcat logger and installs a SIGABRT handler that keeps
Android's crash-loop throttle from blocking your watchdog. There is no container
object to hold — capabilities are reached directly through their feature-gated
modules. On non-Android targets `init` just consumes the config.

### 3. Call a capability

Each capability is a free-function module gated behind its Cargo feature.

```rust
// `scanner`
let value = mobile_sentinel::scanner::scan()?;

// `media_picker` — use this instead of <input type="file"> in a WebView
let path = mobile_sentinel::media_picker::pick_file(&["audio/*"])?;

// `audio`
let handle = mobile_sentinel::audio::play("/path/to/sound.mp3", /* looping */ false)?;

// `haptics`
mobile_sentinel::haptics::vibrate(std::time::Duration::from_millis(200));
```

On host (non-Android) builds these return
`Err(SentinelError::PlatformUnsupported { .. })` or a safe default (`false`,
`0`, `None`), so your code still compiles and runs in desktop tests.

### 4. Or take the whole alarm backend

```rust
use mobile_sentinel::{AlarmKit, AlarmKitConfig, AlarmSpec, ContextStore, app_files_dir};
use std::sync::Arc;

let store = Arc::new(ContextStore::new(app_files_dir().join("sentinel/context")));

let kit = AlarmKit::install(AlarmKitConfig {
    store,
    sink: firing_sink,            // AndroidFiringSink on device, None on host
    sound_resolver,               // a SoundLibrary, or None
    alarm_class_config,           // your channel ids, activity FQCN, kiosk knobs
});

let id = kit.create(AlarmSpec { /* schedule, sound, snooze, … */ ..Default::default() })?;
kit.snooze(&id)?;
kit.dismiss(&id)?;
```

### 5. Build the APK

After your mobile framework (Dioxus, Tauri, …) generates the Android project,
point the bundled binary at it:

```bash
# Single consumer:
cargo run -p mobile-sentinel --bin build_sentinel -- --activity com.example.app.MainActivity

# Workspace with more than one consumer — scope to the app (derives the
# capability set from that app's Cargo.toml features):
cargo run -p mobile-sentinel --bin build_sentinel -- --app myapp
```

This wires `:sentinel-core` plus only the Gradle modules of the capabilities you
enabled, applies the activity attributes lock-screen behavior needs, copies your
icon and assets, and runs Gradle. The output APK lands at
`app/build/outputs/apk/debug/app-debug.apk`.

## Initialization

```rust
pub fn init(config: InitConfig);

pub struct InitConfig {
    pub logger: bool,                 // install android_logger (logcat). Default: true
    pub crash_loop_mitigation: bool,  // install SIGABRT → _exit(0) handler. Default: true
    pub log_tag: String,              // logcat tag. Default: "MobileSentinel"
    pub log_level: log::LevelFilter,  // max level. Default: Debug
}
```

`InitConfig::default()` is what you want in production. The SIGABRT handler is
load-bearing: when a process aborts, Android's `RestartingService` throttle
exponentially backs off restarts. Translating SIGABRT into a clean `_exit(0)`
keeps the `:sentinel` watchdog able to revive the main process immediately.

```rust
mobile_sentinel::init(mobile_sentinel::InitConfig {
    log_tag: "MyApp".to_string(),
    ..Default::default()
});
```

## The FiringSink seam

The single injectable platform interface the recipe engine drives. Six methods,
no god-object:

```rust
pub trait FiringSink: Send + Sync {
    fn start_firing(&self, req: &FireRequest) -> bool;   // engage the whole firing surface
    fn stop_firing(&self, instance_id: &str);            // tear it down (idempotent)
    fn pause_audio(&self);                               // quiet audio only
    fn schedule_exact_alarm(&self, req: &ExactAlarmRequest) -> bool;
    fn cancel_exact_alarm(&self, alarm_id: &str);        // idempotent
    fn system_default_sound_uri(&self) -> String;
}
```

Two implementations ship: `AndroidFiringSink` (JNI, on device) and
`MockFiringSink` (records every call, for engine tests). Install the process-wide
sink once with `install_firing_sink(Arc::new(...))`.

> **Pause vs. Resume.** `pause_audio` only quiets the sound. Bringing the alarm
> back is a *full re-engage* via `start_firing` — there is no `resume_audio`. In
> the recipe layer `reengage` and `resume` are aliases that both route through
> `Trigger::Resume`. This keeps the firing surface built in exactly one place.

## The recipe engine and AlarmKit

`recipes` (the engine) is the reusable state-machine framework. A **Recipe** is a
state machine with a typed **Context** (persisted via `ContextStore`), a set of
**Triggers** that drive transitions, and side effects expressed through
`FiringSink`. You register a recipe once and drive it through a single entry
point:

```rust
register_recipe(MyRecipe::new())?;
dispatch_trigger(&store, Trigger::Fire, &id)?;
```

`dispatch_trigger` enforces the **LoadContext-on-every-trigger** invariant: it
reloads the Context from disk before every transition, so a process restart at
any point is safe. Don't add paths that mutate Context outside dispatch, and
don't cache Context across a transition.

`alarm-kit` is a **prebuilt recipe wrapper** over that engine. It owns no engine
logic — it composes `recipes` + `firing` + `sound-library` + `jobs` and exposes a
clean lifecycle handle (`create`, `update`, `delete`, `snooze`, `dismiss`,
`mark_solved`, `pause`, `resume`, `on_startup`, inspection). The `Trigger` and
the engine live behind `recipes`, *not* behind `alarm-kit`, so you can write your
own recipe without pulling AlarmKit in.

Full signatures, the `AlarmSpec`/`AlarmClassConfig` fields, the `Trigger`
catalogue, the `SessionState` machine, snooze escalation, and the recurrence
`Schedule` types are all in **[docs/API.md](docs/API.md)**.

## Architecture: two processes, one filesystem

Two Android processes share the filesystem as their IPC channel:

- **MAIN** owns the UI + Rust logic + AlarmKit/recipes + the `FiringSink` (which
  calls Kotlin) + every capability.
- **`:sentinel`** is a dumb job guardian. It polls `<files>/sentinel/jobs/*.json`
  and, for each active job, starts MAIN if it's dead or sends a heads-up
  broadcast if it's alive. It knows nothing about alarms — only "some jobs need
  MAIN alive."

Kotlin executes; Rust decides. The state machine writes `SessionState` to the
`ContextStore`; both processes read the same files. That's the whole contract.

## Building the APK (build_sentinel)

`build_sentinel` is the bundled CLI that turns your framework-generated Android
project into a wired, trimmed APK. It reads a small `sentinel.toml`:

```toml
[android]
activity = "dev.dioxus.main.MainActivity"   # required (or pass --activity)
icon = "path/to/icon.webp"                  # optional — copied to all mipmap densities
assets = ["path/to/sounds"]                 # optional — copied into APK assets/
screen_orientation = "portrait"             # optional — default lock for main activity (see display capability for runtime per-screen overrides)
```

Capabilities are **not** declared here — they come from your Cargo features.
`build_sentinel` derives the manifest permissions/components and the set of
Gradle capability modules to compile from the exact features the `.so` was built
with.

Flags:

- `--activity <FQCN>` — the entry activity (overrides `sentinel.toml`).
- `--app <name>` — in a multi-consumer workspace, scope the project,
  `sentinel.toml`, and capability set to `apps/<name>` (derived deterministically
  from that app's `Cargo.toml` feature list).

`build_sentinel` warns prominently when a **policy-sensitive** capability
(`sms`, `phone`, `location`, `device_admin`, `contacts`, `calendar`,
`accessibility`) is declared, so you make an informed choice before shipping.

## Platform support

Android is the real target (`aarch64-linux-android`, compileSdk 34, minSdk
24/26). Every capability also has a **host fallback** so the crate builds and
tests on desktop: capability functions return
`SentinelError::PlatformUnsupported` or a safe default (`false`, `0`, `None`).
This is what lets the recurrence engine, snooze policy, state store, recipe
engine, and AlarmKit be unit- and property-tested on your dev machine with no
device attached. An iOS backend can later implement `FiringSink` and the
capability JNI seams without consumer changes.

## Errors

```rust
pub enum SentinelError {
    PlatformUnsupported { platform: Platform, feature: String, fallback_suggestion: Option<String> },
    RuntimeError { code: ErrorCode, message: String },
}
```

Capability and host errors are `SentinelError`. The recipe layer has one
`RecipeError`; the building blocks have their own shallow error types
(`StoreError`, `SoundError`, `JobGuardianError`, `RecurrenceError`). Errors are
kept deliberately shallow — no deep nesting to unwrap.

Host fallbacks are constructed through `SentinelError::unavailable("module::fn")`
so every capability reports the same shape when called off-device.

## Testing

Host-side checks must stay green after any change:

```bash
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-features      # warnings treated as errors
cargo fmt --all -- --check
```

Verify the feature tiers compile and test independently — that gating is the
whole product:

```bash
cargo test  -p mobile-sentinel                    # no features = kernel-only floor
cargo test  -p mobile-sentinel --features recipes # the recipe engine alone
cargo test  -p mobile-sentinel --features alarm-kit
cargo clippy -p mobile-sentinel --all-features
```

Invariant tests guard the contract: `tests/generic_strings.rs` (no app-specific
strings in the crate), `tests/correctness_properties.rs`
(LoadContext-on-every-trigger), and `tests/manifest_process_topology.rs` (which
components run in `:sentinel`).

On-device behavior is **not** covered by host tests. When a change is only
host-verified, a real `build_sentinel` install on a device is still needed to
confirm device behavior.

## Adding a capability

The capability id (`lower_snake_case`) is the spine. Touch every layer or a
drift test / the manifest will be wrong:

1. `Cargo.toml [features]` — add `<feat> = []` (feature name = kebab form of the
   id).
2. `src/build/capability_ids.rs` — add the id to `CAPABILITY_IDS` (and to
   `FIRING_BUNDLE_IDS` if it's a firing sub-feature).
3. `src/features/<cap>.rs` — the gated module (the only door). Follow the
   `android_or!` / `#[cfg(target_os = "android")]` host-fallback pattern;
   per-capability JNI goes in a private `mod android` using `with_jni_class` /
   `jni_str`.
4. `src/features/mod.rs` + `src/lib.rs` — gated `pub mod` + crate-root re-export.
5. `src/build/registry.rs` — a `CAPABILITIES` row (permissions, components,
   `kotlin_sources`, `policy_sensitive`).
6. `android/caps/<id>/` — the Kotlin Gradle module + `build.gradle.kts`.

The `capability_ids_match_capabilities_table` test fails the build if the id list
and the `CAPABILITIES` table disagree.

## API reference

**[docs/API.md](docs/API.md)** is the complete catalogue of everything a consumer
can call: every capability with its module, entry points, and permissions; the
building blocks (`StateStore`, `SoundLibrary`, Job Guardian, recurrence, snooze);
the recipe engine (`Trigger`, `Recipe`, dispatch, registry, `ContextStore`); and
AlarmKit (`AlarmKit`, `AlarmSpec`, `AlarmClassConfig`, `SessionState`).

## License

MIT. See [LICENSE](LICENSE).

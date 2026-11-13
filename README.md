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
   FiringSink seam               what the engine drives
      │
      ▼
   Kotlin (:sentinel-core + per-capability Gradle modules) over JNI
```

You can stop at any layer: use one capability and nothing else, use the state
store on its own, write your own `Recipe` against the engine, or just take
AlarmKit and get a finished alarm backend.

**The floor (the "husk").** With *no* features you get only the
`:sentinel-core` kernel — a context holder, an activity tracker, and the JNI
bridge. Nothing is callable.

## How feature gating works

Capabilities are **Cargo features** on the mobile-sentinel dependency. That is
the single, compile-enforced source of truth.

```toml
[dependencies]
mobile-sentinel = { path = "../mobile-sentinel", features = [
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
mobile-sentinel = { path = "../mobile-sentinel", features = [
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
// ... or with original names, several types, multi-select (also on Windows)
let files = mobile_sentinel::media_picker::pick_files(&["image/*", "video/*"], true)?;

// `recorder` — voice notes (Android + Windows)
mobile_sentinel::recorder::start(&recordings_dir)?;
let note = mobile_sentinel::recorder::stop()?; // Recording { path, duration_ms }

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
use mobile_sentinel::{AlarmKit, AlarmSpec};

let kit = AlarmKit::install_on_platform(
    bundled_sounds,               // where your sounds are
    custom_sounds,
    alarm_class_config,           // your channel ids, activity FQCN, kiosk knobs
);

// What runs when the app is woken with no UI: after a boot, an update, a clock change.
fn on_wake() { /* set up, and arm what has to ring */ }
mobile_sentinel::wake_hook!(on_wake);

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
    pub clean_process_exit: bool,     // framework exit() → _exit(0), skips static destructors. Default: true
    pub log_tag: String,              // logcat tag. Default: "MobileSentinel"
    pub log_level: log::LevelFilter,  // max level. Default: Debug (debug builds) / Info (release)
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
    fn stop_firing(&self, instance_id: &str);            // end this alarm's ring (idempotent)
    fn release_takeover(&self);                          // nothing rings any more: give the screen back
    fn pause_audio(&self);                               // quiet audio only
    fn schedule_exact_alarm(&self, req: &ExactAlarmRequest) -> bool;
    fn cancel_exact_alarm(&self, alarm_id: &str);        // idempotent
    fn system_default_sound_uri(&self) -> String;
}
```

Two implementations ship: `AndroidFiringSink` (JNI, on device) and
`MockFiringSink` (records every call, for engine tests). `AlarmKit::install_on_platform`
wires up the Android one; `AlarmKit::install` takes your own.

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
- **`:sentinel`** has no UI framework and runs no Rust, so nothing that ends the UI
  can touch it. The alarm receiver and the ring service run here: an alarm plays from
  a plan Rust wrote when it armed it, before MAIN is up. So does the job guardian. It
  polls `<files>/sentinel/jobs/`, and for each active job starts MAIN if it's dead or
  sends a heads-up broadcast if it's alive.

Kotlin executes; Rust decides. The state machine writes `SessionState` to the
`ContextStore`; both processes read the same files. That's the whole contract.

## Building the APK (build_sentinel)

`build_sentinel` is the bundled CLI that turns your framework-generated Android
project into a wired, trimmed APK. It reads a small `sentinel.toml`:

```toml
[android]
activity = "dev.dioxus.main.MainActivity"   # required (or pass --activity)
project = "android"                         # optional — the Gradle project to build (or pass --project); else the one `dx build` generated
icon_background = "F7F7F7"                  # optional — makes the icon adaptive, on this colour
window_background = "F5F6F9"                # optional — the window and splash screen until the page paints (else white),
window_background_dark = "101218"           #   and in the phone's dark mode
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
- `--project <dir>` — the Android project to build, for an app that does not use
  Dioxus (overrides `project` in `sentinel.toml`; relative to the current
  directory). The dir is the Gradle root, the one that holds `app/` and a Groovy
  `settings.gradle` (the capability modules are added to that file; a project with
  only `settings.gradle.kts` is refused). Your own build puts the manifest, the
  native libraries and the Java/Kotlin sources there. Without the flag or the key,
  nothing changes: the project under `target/dx` is used.
- `--app <name>` — in a multi-consumer workspace, scope the project,
  `sentinel.toml`, and capability set to `apps/<name>` (derived deterministically
  from that app's `Cargo.toml` feature list).

How the capability set is found: `mobile-sentinel` keeps the list of the features it was built with
inside the app's `.so` (the string `mobile-sentinel-capabilities:v1[...]`), and `build_sentinel` reads it
from the libraries it packages (`jni_libs` of `sentinel.toml`, or the project's `app/src/main/jniLibs`).
It does not matter where `CARGO_TARGET_DIR` points. A library built with an older mobile-sentinel has no
list; for that one `build_sentinel` still scans `target/` as before.

### Declaring the manifest (no XML)

With `generate = true`, an app declares everything Android needs in `sentinel.toml` and
`build_sentinel` writes the `AndroidManifest.xml` and the `res/xml/` files the declarations point to.
The tables are `[declare.*]` (not `[android.*]`: the flat keys of `[android]`, like `activity` and
`manifest`, cannot share a name with a table). A complete example:
[`examples/declarations/sentinel.toml`](examples/declarations/sentinel.toml).

```toml
[android]
generate = true
application_id = "com.example.app"
rust_lib = "main"                       # the components below are handled by libmain.so

[declare]
permissions = ["INTERNET", "POST_NOTIFICATIONS"]   # bare names get android.permission.

[[declare.permission]]                  # <uses-permission> with attributes
name = "android.permission.WRITE_EXTERNAL_STORAGE"
maxSdkVersion = 28

[declare.application]                   # any attribute of <application>
allowBackup = false

[[declare.activity]]                    # also service, receiver, provider, activity_alias
name = ".MainActivity"
launcher = true                         # MAIN + LAUNCHER filter, exported
launchMode = "singleTask"               # every other key is an android: attribute as written
configChanges = ["orientation", "screenSize"]   # a list is joined with |
native_lib = "main"                     # android.app.lib_name meta-data

[[declare.service]]
name = ".Sync"
exported = false
foregroundServiceType = ["dataSync", "specialUse"]
special_use = "Keeps the local relay running"   # PROPERTY_SPECIAL_USE_FGS_SUBTYPE
lib = "main"                            # mobilesentinel.lib: the code of this service is Rust

[[declare.service]]                     # count: one service per process, no generator script
name = ".G{i}"
process = ":g{i}"
exported = false
count = 128

[[declare.receiver]]
name = ".Admin"
admin_policies = ["force-lock", "wipe-data"]   # or "all"; writes res/xml, sets BIND_DEVICE_ADMIN
[[declare.receiver.intent_filter]]
actions = ["android.app.action.PROFILE_PROVISIONING_COMPLETE"]

[declare.boot_start]                    # preset: BOOT_COMPLETED receiver (+ RECEIVE_BOOT_COMPLETED)
receiver = ".Boot"
```

What can be declared (all keys under `[declare]`):

- `permissions`, `permission` (`name`, `maxSdkVersion`, `usesPermissionFlags`, ...), `feature` (`name` or
  `glEsVersion`, `required`), `define_permission`, `permission_group`, `permission_tree`.
- `application` (every `<application>` attribute, plus `meta_data` and `property`), `uses_library`,
  `uses_native_library`, `queries` (`packages`, `providers`, `[[declare.queries.intent]]`).
- `activity`, `activity_alias`, `service`, `receiver`, `provider`: any attribute (`exported`, `process`,
  `permission`, `directBootAware`, `foregroundServiceType`, `launchMode`, `theme`, `configChanges`,
  `taskAffinity`, `isolatedProcess`, `enabled`, `authorities`, ...), and `intent_filter` (`actions`,
  `categories`, `data` with `scheme`/`host`/`path*`/`mimeType`, filter attributes), `meta_data` and
  `property` (`name` and `value` or `resource`), `layout`, `grant_uri_permission`, `path_permission`.
  A key with a colon (`"tools:node"`) is written as it is. Names are matched to the lists on
  developer.android.com only to warn about typos, so a new attribute works the day Android has it.
- `count = N` repeats a table N times; `{i}` (0 to N-1) is replaced in every string.
- Generated resources: `admin_policies` (any of `force-lock`, `reset-password`, `limit-password`,
  `watch-login`, `wipe-data`, `expire-password`, `encrypted-storage`, `disable-camera`,
  `disable-keyguard-features`, or `"all"`), `accessibility = { ... }` on a service (the accessibility
  config), `file_paths = [{ kind, name, path }]` on a provider, `[declare.network_security]`, and any
  other `res/xml` file as a tree with `[[declare.resource_xml]]` (`path`, `tag`, `attrs`, `children`).
  `[[declare.element]]` does the same for any other manifest element (`parent = "application"` or the
  manifest).
- Presets built on the same tables: `[declare.work_profile]` (`admin`, `compliance`, `policies`, `lib`;
  a profile owner, with `MODIFY_QUIET_MODE`), `[declare.boot_start]` (`receiver`, `locked`,
  `package_replaced`, `actions`), `[[declare.process_pool]]` (`prefix`, `count`, `kind`,
  `process_prefix`, other keys as attributes).

The build helps and checks: a foreground service type adds `FOREGROUND_SERVICE` and
`FOREGROUND_SERVICE_<TYPE>`; a `BOOT_COMPLETED` or `LOCKED_BOOT_COMPLETED` filter adds
`RECEIVE_BOOT_COMPLETED` (likewise `SMS_RECEIVED` and `NEW_OUTGOING_CALL`); a permission that stops
below `min_sdk` stays out. It prints a warning for what Google Play reviews (`QUERY_ALL_PACKAGES`,
`specialUse` services, device admin, accessibility, SMS and call log, all-files access, exact alarms,
...) and refuses what Android rejects: an unknown foreground service type, `specialUse` without a
subtype, an intent filter without `exported`, a filter without an action or with a host but no scheme,
a bad process name, `launchMode` or `screenOrientation`, two components of one name, `count` without
`{i}`, `value` and `resource` together.

With `native_lib` the standard NativeActivity launcher stays unless an activity of that name is
declared. `manifest = "..."` (an own manifest file) still works as before and is the last resort; it
cannot be combined with `[declare]`. The Rust code of a component comes from `lib` / `rust_lib` as
before (activities, services and receivers; a `provider` needs a class of its own).

### Faster and smaller builds

`build_sentinel` leaves Gradle's daemon, build cache and configuration cache on; the generated
`gradle.properties` turns them on. A rebuild with only a new `.so` takes about 4 s instead of 40 s. The
first build after a change of features (so of `settings.gradle`) is a normal one.

Ways an app can make its own release build smaller (measured on a Slint app, in the app's repository,
not in this crate):

```toml
# Cargo.toml of the app
[profile.release]
opt-level = "s"
lto = "thin"
codegen-units = 1
panic = "abort"      # about -8% of the .so (the unwind tables). A panic ends the process.
```

`panic = "abort"` has one effect here: `wake_hook!` catches a panic of the hook with `catch_unwind` and logs
it; under `abort` there is nothing to catch and the process ends (Android starts it again for the next
wake). An app whose wake hook must survive a panic keeps `unwind`.

In `sentinel.toml`, `legacy_jni_packaging = true` stores the native library compressed in an **APK** and
extracts it on install: a sideloaded or GitHub APK shrinks by about half, with no start-up cost seen. It is
ignored for an AAB (`--aab`), where Play decides.

`build_sentinel` warns prominently when a **policy-sensitive** capability
(`sms`, `phone`, `location`, `device_admin`, `contacts`, `calendar`,
`accessibility`) is declared, so you make an informed choice before shipping.

## Platform support

Android is the real target (`aarch64-linux-android`, modules at compileSdk 34,
minSdk 24; what an app runs on is its own `minSdk`). Every capability also has a **host fallback** so the crate builds and
tests on desktop: capability functions return
`SentinelError::PlatformUnsupported` or a safe default (`false`, `0`, `None`).
This is what lets the recurrence engine, snooze policy, state store, recipe
engine, and AlarmKit be unit- and property-tested on your dev machine with no
device attached. An iOS backend can later implement `FiringSink` and the
capability JNI seams without consumer changes.

**Windows** has real backends for the capabilities a desktop app needs, each
pulled in only by its feature:

| Capability | Windows backend |
|---|---|
| `media_picker::pick_files` | native file dialog (`rfd`) |
| `recorder` | microphone via WASAPI (`cpal`), saved as WAV (`hound`) |
| `share::open` / `share::save_to_downloads` | default program (`opener`) / Downloads folder |

Everything else returns the same host fallback as above.

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

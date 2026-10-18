# Changelog

All notable changes to **mobile-sentinel** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project aims to adhere to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Pre-1.0, on-disk formats (StateStore / ContextStore JSON, job files) ship as a
single clean migration — no back-compat shims. Uninstall/reinstall on a format
change.

## [Unreleased]

### Added

- `background_service` capability: a quiet foreground service (type
  `remoteMessaging`, lowest-importance notification) that keeps the app's
  process and connection alive after the UI is closed, restarts after boot or
  an update, posts message/call notifications whose tap hands a string back to
  the app (`take_open_request`), and routes the Back button to the app so it can
  go to the background instead of being destroyed. The host app can implement
  `nativeStart` / `nativeOnBack` JNI hooks. Purely additive.
- `webview_media` capability: declares CAMERA, RECORD_AUDIO and
  MODIFY_AUDIO_SETTINGS so `getUserMedia` works in the app WebView (live
  voice/video calls). Manifest only; no other app's output changes.
- `background_service::exit_app()` closes the app for real (screen and process).
- `link_scheme` in `sentinel.toml`: clicking `scheme://...` opens the app; the link
  arrives as `take_open_request()` = `link:<uri>` (needs `background_service`).
  Open requests now also survive the process restart described below.
- Back is now always routed to an app that implements `nativeOnBack` (not only while
  the background service is on), so it can close a picture or go back a screen
  instead of the system finishing the activity. Apps without the hook are unchanged.

## [0.2.2] - 2026-10-16

Media capabilities for messenger-style apps, and the first Windows backends.
Purely additive: no existing function, feature or manifest output changes
(an app with the same features gets a byte-identical manifest).

### Added

- `media_picker::pick_files(mimes, multiple)` → `Vec<PickedFile{path, name,
  mime, size}>`: keeps the original file name, supports several MIME types
  and multi-select. Android (new `pickFilesInfoBlocking`) and **Windows**
  (native dialog via `rfd`).
- `camera::capture_video()` (Android): record with the system camera app,
  returns an `.mp4` path. Copes with camera apps that ignore `EXTRA_OUTPUT`.
- New `recorder` capability (voice notes): `start(dir)`, `level()`,
  `is_recording()`, `stop() -> Recording{path, duration_ms}`, `cancel()`,
  `has_permission()`, `request_permission()`. Android: `MediaRecorder`
  (AAC `.m4a`, declares `RECORD_AUDIO`). **Windows**: `cpal` + `hound` (WAV).
- `share::open(path, mime)` (Android `ACTION_VIEW`, Windows default program)
  and `share::save_to_downloads(path, name, mime)` (Android MediaStore
  Downloads, Windows Downloads folder).
- The `share` capability now declares its own FileProvider
  (`<package>.sentinelshareprovider`, subclass so it coexists with the
  camera provider), so `share::file` works without host setup. It also
  declares `WRITE_EXTERNAL_STORAGE` with `maxSdkVersion=28` for saving to
  Downloads on Android 9 and older.

### Fixed

- The build registry's feature-graph parser treated `dep:` entries in
  `[features]` as features.

## [0.2.1] - 2026-10-16

### Fixed

- `build_sentinel` now removes `android:extractNativeLibs` instead of setting
  it to `false` (AGP warned it "should not be specified" in source manifests;
  for minSdk >= 23 AGP already defaults to not extracting).
- The manifest attribute remover kept the attribute's value and closing quote
  (`landscape"`), which would corrupt the manifest when replacing an existing
  attribute such as `screenOrientation`.

## [0.2.0] - 2026-10-16

Reliability release: a ringing alarm now survives OEMs that kill every app
process at once (verified on ColorOS), plus size and battery work.

### Breaking

- `InitConfig` has a new field `clean_process_exit` — struct literals without
  `..Default::default()` must add it.
- `RecipeError::ChallengesUnsolved` — new variant; exhaustive matches must
  handle it.
- `AlarmKit::dismiss` on a firing alarm with challenges now returns
  `Err(Recipe(ChallengesUnsolved))` until `mark_solved` is called (the gate was
  previously only enforced by the UI).

### Added

- Firing watchdog: while an alarm rings, a separate `<instance>#watchdog`
  exact alarm is kept 6 s ahead (re-armed every 2 s). If MAIN and `:sentinel`
  are both killed (OEM swipe cleaners), AlarmManager wakes `:sentinel` again.
  It never activates a job, so a stale watchdog cannot re-ring a dismissed
  alarm.
- The foreground service reopens the active job's activity when Android
  restarts it after a kill (from `onCreate`; some OEMs never deliver the
  null-intent `onStartCommand`), and a heads-up reaching a MAIN process without
  UI now launches the activity.
- Kiosk: also blocks legacy `KEYCODE_BACK` (OEMs that disable predictive back
  ignore `OnBackInvokedCallback`).
- `display::safe_area_insets()` and `display::set_light_system_bars()` for
  edge-to-edge layouts (Android 15+ / targetSdk 35).
- `InitConfig::clean_process_exit` (default on): a framework `exit()` ends the
  process with `_exit(0)` instead of racing static destructors (the "pthread
  mutex destroyed" native abort when the activity is destroyed).
- `StateStore::change_marker()`: one `stat` to tell whether any record changed.

### Changed

- Real alarms are armed with `setAlarmClock()` (Doze-exempt, shown as the next
  alarm); the watchdog uses `setExactAndAllowWhileIdle`.
- Job and state files are written atomically (unique temp file + `sync_all` +
  rename), including the `:sentinel` receiver's job activation.
- `app_files_dir()` caches its result instead of doing a JNI round trip per call.
- Default log level is `Info` in release builds (`Debug` in debug builds).
- `build_sentinel`: sets `extractNativeLibs="false"` and removes previously
  copied assets that were deleted from the source.
- Non-test locks recover from poisoning instead of panicking.

### Fixed

- `StateStore` docs claimed cross-process write safety; it is single-writer
  per process (as used).

## [0.1.4]

The first documented baseline of the SDK. Everything below describes the surface
as it stands today; subsequent releases will track changes against it.

### Added

- **Feature-gated capability layer.** Each Android capability is its own Cargo
  feature and its own door (`mobile_sentinel::<cap>`): `scanner`, `camera`,
  `haptics`, `audio`, `overlay`, `permissions`, `sensors`, `torch`, `display`,
  `battery`, `screen_pin`, `foregrounding`, `media_picker`, `clipboard`,
  `share`, `secure_storage`, `sms`, `phone`, `network`, `location`, `maps`,
  `biometric`, `device_admin`, `contacts`, `calendar`, `dismiss_guard`,
  `notifications`, `file_system`, `accessibility`. A disabled feature compiles
  no Rust, contributes no manifest permission/component, and wires no Kotlin
  Gradle module; calling it is a compile error.
- **Host fallbacks for every capability** so the crate builds and tests on
  desktop — calls return `SentinelError::PlatformUnsupported` or a safe default.
- **Building blocks**, each its own feature with standalone value:
  - `state-store` — `StateStore<T>`: atomic, per-id, revisioned JSON store.
  - `sound-library` — `SoundLibrary` / `SoundId`: bundled + custom + system
    sound resolution with missing-file fallback.
  - `jobs` — the cross-process Job Guardian (`register_job`, `activate_job`,
    `deactivate_job`, `complete_job`, …).
- **Recurrence engine** (`recipes`) — DST-correct `next_fire` over `Schedule`
  (`OneTime` / `Weekdays` / `Monthly`; `Cron` reserved) with `WeekdaySet`.
- **Snooze policy** (`recipes`) — `SnoozePolicy` with constant / `Linear` /
  `Exponential` / `Custom` escalation.
- **Recipe engine** (`recipes`) — the `Trigger` catalogue, the `Recipe` trait,
  `dispatch_trigger` (enforcing LoadContext-on-every-trigger), the global recipe
  registry, and the typed `ContextStore` (`StateStore<ContextRecord>`).
- **FiringSink seam** (firing sub-features) — the 6-method `FiringSink` trait
  (`start_firing`, `stop_firing`, `pause_audio`, `schedule_exact_alarm`,
  `cancel_exact_alarm`, `system_default_sound_uri`), the `FireRequest` /
  `ExactAlarmRequest` types, `install_firing_sink` / `firing_sink`, and the
  `MockFiringSink` recorder. Split into granular sub-features (`wake-lock`,
  `firing-audio`, `firing-vibration`, `foreground-service`, `exact-alarm`,
  `kiosk`, `full-screen-intent`) bundled by `firing`.
- **AlarmKit** (`alarm-kit`) — a prebuilt recipe wrapper over the engine + firing
  + sound + jobs. `AlarmKit::install`, the full lifecycle (`create`,
  `create_with_id`, `update`, `delete`, `snooze`, `dismiss`, `mark_solved`,
  `pause`, `resume`/`reengage`, `on_startup`, `handle_job_heads_up`), inspection
  (`list`, `get`, `next_fire`, `current_session`, …), and the `AlarmSpec` /
  `AlarmClassConfig` / `SoundResolver` surface.
- **Initialization** — `init(InitConfig)` (logcat logger + SIGABRT crash-loop
  mitigation on Android).
- **Cross-platform callbacks** — `callbacks::on_boot_completed`,
  `callbacks::on_job_heads_up` (no-ops on host).
- **Utilities** — `app_files_dir()` and `AssetExtractor` for bundled-asset
  extraction.
- **`build_sentinel` CLI** — derives the Android manifest permissions/components
  and the set of Gradle capability modules to compile from the exact Cargo
  features the `.so` was built with. Supports `--activity` and, for
  multi-consumer workspaces, `--app <name>`. Warns on policy-sensitive
  capabilities.
- **Two-process architecture** — MAIN (UI + Rust logic + FiringSink) and
  `:sentinel` (a generic, alarm-agnostic job guardian) sharing the filesystem as
  IPC.
- **Documentation** — rewritten `README.md` and a complete API reference at
  `docs/API.md`.

[Unreleased]: https://github.com/fidderr/mobile-sentinel/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/fidderr/mobile-sentinel/releases/tag/v0.2.1
[0.2.0]: https://github.com/fidderr/mobile-sentinel/releases/tag/v0.2.0
[0.1.4]: https://github.com/fidderr/mobile-sentinel/releases/tag/v0.1.4

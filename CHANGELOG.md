# Changelog

All notable changes to **mobile-sentinel** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project aims to adhere to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Pre-1.0, on-disk formats (StateStore / ContextStore JSON, job files) ship as a
single clean migration — no back-compat shims. Uninstall/reinstall on a format
change.

## [Unreleased]

### Changed

- An app gets what it asked for, also in its bundle. `ring`: what a ring is made of left
  `core` for a module of its own, so an app without alarms does not carry it.
  `build_sentinel` takes out Material, which the Dioxus template adds and nothing here
  uses, and has a release drop unused resources. An adaptive icon stores its picture once.
  A permission that stops below the app's `minSdk` stays out of its manifest. Only the
  modules that use androidx depend on it.
- `health`: the maker's "auto-start" cannot be read, so it is no longer guessed from the
  phone's brand: `boot_signal_missed` says the phone restarted and the signal never came.
- `sensors`: the step counter no longer asks for the fastest rate.

### Removed

- `install_firing_sink` and `firing_sink`: nothing read what they set. Pass a sink to
  `AlarmKit::install`.
- `AlarmKit::handle` and `AlarmKit::next_fire_time`: `AlarmKit::next_fire` has it.
- `tokio`, `futures-core` and `futures-channel`, which nothing used.

### Fixed

- `keep_event_loop_awake`, for an app to call from its UI's thread. tao on Android loses a wake
  that comes in together with a window event, as when another Activity closes, and a Dioxus UI
  then stands still until something like a touch.
- An exact alarm's broadcast goes through the foreground queue. In the background queue, up to
  Android 13, it waited its turn behind whatever else was going on: 13 seconds right after a boot.
- `scanner`, `media_picker`, `camera`: what was scanned, picked or taken after a minute or two was
  lost, with the activity still open and the app no longer waiting. They wait 30 minutes, and 10
  for a photo.
- `scanner`: cancelling said so twice, the second time when the activity was destroyed. A scan
  started within that moment ended at once, and what it scanned was lost. From Android 13 on Back
  said so only then, which could leave the app waiting for seconds. It says so once, at finishing.
- `display::safe_area_insets` also says how far the keyboard covers the content
  (`keyboard`): an edge-to-edge window does not shrink for it.
- `screen_pin`: `pin` and `unpin` looked for an Activity in the application context, which
  never is one, so they could not work. They use the resumed Activity.
- `biometric`: `authenticate` had the same problem, and would have called a prompt on screen
  a success. It waits for the answer now, and is `Ok` only if the user was recognised.
- `phone`: opening the dialer needs no `CALL_PHONE`, so it is gone, and with it the Play
  review. `is_in_call` reads the audio mode, as the call state wants a permission from
  Android 12 on.
- `secure_storage` is private to the app but not encrypted, whatever the docs said.
- A ring whose processes all died came back without its screen, as on ColorOS when an app
  is swiped away: nothing in the new `:sentinel` was guarding.
- The guardian could stop just as a job became active.
- `background_service` and `exact_alarm` had a class of the same name, so an app could not
  have both.
- A reminder's notification icon was the app's icon, which shows as a blob in the status bar.
- `display`: brightness and keep-screen-on did nothing, since the app context stopped
  being an Activity. A repeated call also saved the override as the original.
- `build_sentinel`: Gradle output of an app's build no longer lands inside this crate.

### Added

- `health::report` and `health::open_settings`: what the phone allows for alarms to
  ring on time, and how to get to where it is changed. `AlarmKit::timing_log` keeps
  a small record of how fast rings were, for testers on other phones.
- `direct_boot = true` in `sentinel.toml`: alarms ring before the first unlock, from
  device-encrypted files. `SentinelBackupActivity` is the alarm's screen then, and
  when the UI keeps failing (`JobConfig::ui_failures_before_backup`); see
  `AlarmClassConfig::backup_holds`.
- `SentinelRearmReceiver`: alarms are armed again with no app open on a boot, an app
  update, a clock or time zone change, through what the app says with
  `mobile_sentinel::wake_hook!`. `AlarmKit::on_wake` is the startup for that case.
- `AlarmKit::install_on_platform`: installs what the platform rings with, so an app
  need not name the Android sink, the sound backend or where the context store lives.
- `AlarmSpec::sound_delay_secs`: vibrate first, then the sound.
- `audio::play_as_alarm`: try out a sound the way the alarm will play it.
- `AlarmKit::snoozed_until`: when a snoozed alarm rings again.
- `home_widget` capability: a home-screen widget with two lines the app sets.
- `icon_background` in `sentinel.toml`: an adaptive launcher icon from a picture.
- `ring_limit_secs` (per alarm, default 30 minutes, `None` = until dealt with): the
  ring service stops an alarm nobody responds to. It counts from the ring's start,
  a touch on the alarm screen, leaving it, the app coming back, or
  `AlarmKit::note_activity`, and a guardian kill of a frozen UI does not count. Rust
  then ends the alarm past the challenge gate; `StartupReport::ring_ends` and
  `AlarmKit::take_ring_ends` tell the consumer.
- `AlarmKit::on_startup` now returns a `StartupReport` and ends rings a reboot
  interrupted (past the challenge gate); a snooze still in the future is kept
  and re-armed. An unchanged alarm is no longer reset to Idle on startup, and
  `update()` with an unchanged spec keeps a snooze in progress.
- `skip_until_unix_ms` (spec + context): skip one occurrence of a repeating alarm.
- `fade_in_secs` (per alarm) and `AlarmClassConfig::min_volume_percent`: the sound
  fades in on the first ring, and the alarm stream is raised to a minimum while
  ringing and restored afterwards.
- `InitConfig::panic_log`: every Rust panic goes to logcat and
  `files/sentinel/last_panic.txt`.
- `jobs::ui_heartbeat()` + `JobConfig::{ui_liveness_timeout_ms, ui_liveness_backoff_cap,
  min_launch_interval_ms}`: the `:sentinel` guardian restarts a frozen UI, and
  throttles launches of MAIN. All policy comes from the job config.
- The Kotlin exact-alarm module records what it armed (`sentinel/armed/`) and the
  boot service replays it after a reboot; it never reads Rust's own files.
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
- `share::open_url(url)` opens a web address (http/https) in the browser: Android `ACTION_VIEW`, Windows the default browser.
- `background_service::exit_app()` closes the app for real (screen and process).
- `link_scheme` in `sentinel.toml`: clicking `scheme://...` opens the app; the link
  arrives as `take_open_request()` = `link:<uri>` (needs `background_service`).
  Open requests now also survive the process restart described below.
- Back is now always routed to an app that implements `nativeOnBack` (not only while
  the background service is on), so it can close a picture or go back a screen
  instead of the system finishing the activity. Apps without the hook are unchanged.

### Changed

- **An alarm rings from `:sentinel`, before any Rust runs.** Rust resolves a plan
  when it arms an alarm (a serialized `FireRequest`, carried by
  `ExactAlarmRequest::metadata_json`) and the alarm receiver starts the ring
  service from it. The foreground service, sound and vibration moved out of MAIN,
  so an exit of the UI can no longer stop the sound, and a ring no longer depends
  on the activity being allowed to start. Call `AlarmKit::refresh_ring_plans` after
  changing a runtime config a plan depends on.
- `FiringSink::start_firing` is safe to repeat: it no longer restarts the sound or
  the fade-in. `stop_firing` ends one alarm's ring; the new `release_takeover` gives
  the screen back once none is left.
- The firing watchdog moved from a Rust thread into the ring service. One watchdog
  covers all rings.
- `firing-audio`, `firing-vibration` and `wake-lock` now imply `foreground-service`.
  `SentinelForegroundServicePrimitives` and the `wake_lock` Kotlin module are gone;
  the `wake_lock` capability only declares the permission. `SentinelRing` (core) is
  the command surface.
- `AlarmKit::current_session` answers with the alarm firing longest, so it does not
  change while it rings.

- A job is active while its marker file `sentinel/jobs/<id>.active` exists, instead
  of a status in the job file. Kotlin only creates the marker and Rust alone writes
  the job file, so saving a job while it fires can no longer lose the fire.
- `StartupReport` also lists `missed` alarms and `finished` one-shot alarms.
  `AlarmClassConfig::late_ring_window_ms` (default 15 minutes) sets how late an
  alarm may still ring when its time passed unnoticed.
- Firing keeps the next occurrence armed, so a recurring alarm survives a reboot
  mid-ring without the app being opened.

### Fixed

- Dismissing one of two ringing alarms silenced the other and released the lock.
- A sound that cannot be played no longer means silence: `fallback_sound_uris` and
  `AlarmClassConfig::fallback_sound_id` are tried in turn, also when playback does
  not advance, and if none works the alarm vibrates.
- Alarm vibration uses the alarm usage, so it gets through silent mode.
- The firing notification is `CATEGORY_ALARM`, not `CATEGORY_CALL`, so Do Not
  Disturb treats it as an alarm. An alarm without a label gets
  `AlarmClassConfig::fallback_title`.
- Volume keys are swallowed during the lock (`kiosk_block_volume`) and the minimum
  volume is applied again every second. The level to restore is kept on disk, so a
  killed process cannot leave the phone loud.
- The screen stays on while the lock is on, and API 26 uses the window flags for
  showing over the lock screen.

- A snooze that ran out with the process dead counted as "ended by a reboot" and
  did not ring. Finishing the UI ends the process, so this hit every snooze.
- Re-arming cancelled the exact alarm first; a kill in between lost the alarm. The
  new wake now simply replaces the old one.
- `update` with an unchanged spec no longer re-arms, and a snooze survives a time
  zone change.
- `delete` tore down the whole firing surface even when another alarm was ringing.
- A failed time zone lookup silently scheduled in UTC, moving alarms by hours. It
  now keeps the last known zone, and a zone the database doesn't know (a custom
  `GMT+01:00`) becomes its fixed offset instead of leaving the alarm unarmed.
- An active job with nothing left to ring is dropped. It kept the guardian
  relaunching the app.
- Triggers are serialised: `dispatch_trigger`'s documented lock did not exist.
- The main activity handles all configuration changes itself. A dark-mode switch,
  font-size change or fold recreated the activity, which ends the process.
- The alarm screen froze after a reboot or force-stop mid-alarm: the boot receiver
  ran in MAIN and started the job guardian there, whose liveness check skipped its
  own pid and relaunched the activity ~10x per second. The receiver now runs in
  `:sentinel`, the guardian counts its own process as MAIN, launches are throttled,
  and kiosk/job state older than the current boot is ignored.
- `start_firing` no longer launches the full-screen intent over a visible UI, and
  `on_startup` engages a firing alarm once instead of twice.

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

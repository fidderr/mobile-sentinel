# Changelog

All notable changes to **mobile-sentinel** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project aims to adhere to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Pre-1.0, on-disk formats (StateStore / ContextStore JSON, job files) ship as a
single clean migration — no back-compat shims. Uninstall/reinstall on a format
change.

## [Unreleased]

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

[Unreleased]: https://github.com/fidderr/mobile-sentinel

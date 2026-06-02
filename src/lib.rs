//! # mobile-sentinel
//!
//! A Rust-first Android SDK. Write your mobile app logic in Rust; Kotlin
//! is only a thin JNI execution layer.
//!
//! ## Overview
//!
//! Every JNI-backed platform capability is its own **feature-gated module**
//! — audio, foreground service, notifications, wake locks, exact alarms,
//! kiosk, full-screen intent, permissions, pickers, scanners, sensors,
//! biometrics, haptics, display, torch, battery, device admin, screen
//! pinning, clipboard, share, secure storage, SMS, phone, network, location,
//! maps, calendar, contacts, and more. Each capability is reachable through
//! exactly one door (e.g. `mobile_sentinel::camera`); enabling its Cargo
//! feature is what makes that door callable, and calling a capability whose feature is not
//! enabled is a compile error. A disabled feature is never compiled and
//! contributes nothing to the manifest, Kotlin, Gradle deps, or native `.so`.
//!
//! The only injectable platform interface is the small [`FiringSink`] trait
//! (six methods) that the Recipe engine drives; on Android it is
//! implemented by `AndroidFiringSink` and in tests by a recorder.
//!
//! On top of those capabilities it ships the Recipe framework (AlarmKit
//! and friends), a ContextStore for durable per-instance state, a generic
//! Job Guardian for cross-process persistence, a DST-correct Recurrence
//! Engine, and a Sound Library.

pub mod build;
/// Recipe Context schema + durable ContextStore. Part of the alarm-kit
pub mod error;
/// Consumer-facing capabilities, each gated behind its own Cargo feature.
/// Grouped under `features/`; re-exported at the crate root below so
/// consumers use short paths like `mobile_sentinel::camera`.
pub mod features;
/// Firing surface seam — the small injectable `FiringSink` interface the
/// recipe engine drives and the platform implements. Foundational (not
/// recipe-specific); compiles when any firing sub-feature is enabled.
#[cfg(firing_enabled)]
pub mod firing;
pub mod init;
pub mod platform;
/// Recipe layer — the recipe engine (`recipes` feature: `Trigger`, the
/// `Recipe` trait, dispatch + registry, `ContextStore`, recurrence, snooze)
/// plus prebuilt recipe wrappers like AlarmKit (`alarm-kit` feature). The
/// engine is the reusable building block; the wrappers own no engine logic.
/// Ships nothing when no recipe feature is enabled.
#[cfg(feature = "recipes")]
pub mod recipes;
pub mod sink_types;
pub mod testing;
pub mod types;
pub mod utilities;

// Standalone building-block features live under `features/`. Re-export them
// at the crate root under their canonical module names so existing paths
// (`crate::state_store`, `crate::sound`, `crate::jobs`) resolve.
#[cfg(feature = "jobs")]
pub use features::jobs;
#[cfg(feature = "sound-library")]
pub use features::sound;
#[cfg(feature = "state-store")]
pub use features::state_store;

/// The Android-backed firing sink — only when a firing surface is enabled.
#[cfg(all(target_os = "android", firing_enabled))]
pub use platform::android::AndroidFiringSink;

/// Android `SoundBackend` — part of the Sound Library subsystem.
#[cfg(all(target_os = "android", feature = "sound-library"))]
pub use platform::android::AndroidSoundBackend;

/// Cross-platform callback registration for system events.
/// Register closures to be called when Android system events occur
/// (alarm fired, boot completed, service lifecycle). On non-Android
/// platforms these are no-ops.
pub use platform::callbacks;

// Consumer-facing capability modules — re-exported at the crate root so
// consumers use short paths (e.g. `mobile_sentinel::camera::scan_barcode`).
// Each is gated by its own Cargo feature inside `features::`.
#[cfg(feature = "accessibility")]
pub use features::accessibility;
#[cfg(feature = "audio")]
pub use features::audio;
#[cfg(feature = "battery")]
pub use features::battery;
#[cfg(feature = "biometric")]
pub use features::biometric;
#[cfg(feature = "calendar")]
pub use features::calendar;
#[cfg(feature = "camera")]
pub use features::camera;
#[cfg(feature = "clipboard")]
pub use features::clipboard;
#[cfg(feature = "contacts")]
pub use features::contacts;
#[cfg(feature = "device_admin")]
pub use features::device_admin;
#[cfg(feature = "dismiss_guard")]
pub use features::dismiss_guard;
#[cfg(feature = "display")]
pub use features::display;
#[cfg(feature = "file_system")]
pub use features::file_system;
#[cfg(feature = "foregrounding")]
pub use features::foregrounding;
#[cfg(feature = "haptics")]
pub use features::haptics;
#[cfg(feature = "location")]
pub use features::location;
#[cfg(feature = "maps")]
pub use features::maps;
#[cfg(feature = "media_picker")]
pub use features::media_picker;
#[cfg(feature = "network")]
pub use features::network;
#[cfg(feature = "notifications")]
pub use features::notifications;
#[cfg(feature = "overlay")]
pub use features::overlay;
#[cfg(feature = "permissions")]
pub use features::permissions;
#[cfg(feature = "phone")]
pub use features::phone;
#[cfg(feature = "scanner")]
pub use features::scanner;
#[cfg(feature = "screen_pin")]
pub use features::screen_pin;
#[cfg(feature = "secure_storage")]
pub use features::secure_storage;
#[cfg(feature = "sensors")]
pub use features::sensors;
#[cfg(feature = "share")]
pub use features::share;
#[cfg(feature = "sms")]
pub use features::sms;
#[cfg(feature = "torch")]
pub use features::torch;

// Core error type.
pub use error::{Platform, SentinelError};
pub use types::{InstanceId, PlaybackHandle};

// Context Store — single source of truth for Recipe instance state. The
// module lives under `recipes::context`; re-exported here so the public path
// `mobile_sentinel::context` (and the flattened type re-exports below) stay
// stable for consumers. Part of the recipe engine.
#[cfg(feature = "recipes")]
pub use recipes::context;
#[cfg(feature = "recipes")]
pub use recipes::context::{
    AlarmClassContext, ContextRecord, ContextStore, RecipeContext, Revision, StoreError,
};

// Generic durable per-instance state store — standalone consumer primitive.
// `Revision`/`StoreError` are re-exported under distinct names to avoid
// clashing with the recipe `context` re-exports of the same type aliases.
#[cfg(feature = "state-store")]
pub use state_store::{Revision as StateRevision, StateStore, StateStoreError, Stateful};

// Recurrence Engine — recipe-engine helper (DST-correct next-fire).
#[cfg(feature = "recipes")]
pub use recipes::recurrence::{next_fire, RecurrenceError, Schedule, WeekdaySet};

// Snooze Policy — recipe-engine helper.
#[cfg(feature = "recipes")]
pub use recipes::snooze::{EscalationPolicy, SnoozePolicy};

// Sound Library — bundled + custom + system-default sound resolution.
#[cfg(feature = "sound-library")]
pub use sound::{SoundBackend, SoundEntry, SoundError, SoundId, SoundLibrary};

// Recipe engine — the `Recipe` trait, its error type, the dispatch boundary,
// and the registry. The reusable building block any recipe is built on.
#[cfg(feature = "recipes")]
pub use recipes::{
    dispatch_trigger, recipe_registry, register_recipe, Recipe, RecipeError, RecipePermission,
    RegistrationError,
};

// AlarmKit — a prebuilt recipe wrapper. Gated behind `alarm-kit`; composes the
// recipe engine above with the firing / sound / jobs building blocks.
#[cfg(feature = "alarm-kit")]
pub use recipes::{
    alarm_class_runtime, AlarmClass, AlarmKit, AlarmKitConfig, AlarmKitError, AlarmKitSession,
    AlarmSpec,
};

// FiringSink — the small injectable firing interface the recipe engine uses.
// Firing types + install/accessor. Foundational (top-level `firing` module);
// compiles when any firing sub-feature is on.
#[cfg(firing_enabled)]
pub use firing::{
    firing_sink, install_firing_sink, ExactAlarmRequest, FireRequest, FiringSink, MockFiringSink,
};

// Capability data types — the structured values capability modules
// (permissions, biometric, network, location, maps, contacts, calendar)
// accept and return.
pub use sink_types::{
    BiometricType, CalendarEvent, ConnectionType, Contact, Coordinate, PermissionState,
};

// Trigger — the typed Recipe state-transition entry point. Part of the
// recipe engine.
#[cfg(feature = "recipes")]
pub use recipes::Trigger;

// Utility types re-exports.
pub use utilities::{app_files_dir, AssetExtractor};

// Sensor access (accelerometer shake detection + step counter) is exposed
// through the feature-gated `sensors` module (see `mod sensors`), not as
// crate-root free functions, so it cannot be used without declaring the
// `sensors` feature.

// Init API re-exports.
pub use init::{init, InitConfig};

// Job Guardian — generic polling-based job persistence for :sentinel.
#[cfg(feature = "jobs")]
pub use jobs::{
    activate_job, complete_job, deactivate_job, get_active_jobs, get_job, jobs_dir, register_job,
    remove_job, Job, JobConfig, JobGuardianError, JobStatus,
};

// Build helper for Android project preparation.
pub use build::helper::{
    prepare_android_project, prepare_android_project_with_capabilities,
    prepare_android_project_with_capabilities_and_orientation,
};

// Capability registry — declarative manifest assembly for build_sentinel.
pub use build::registry::{assemble, capability, Capability, ManifestInjection, CAPABILITIES};

//! Consumer-facing capabilities.
//!
//! Each submodule here is the **only** sanctioned entry point for one
//! capability, gated behind its own Cargo feature. A capability that a
//! consumer has not enabled is not compiled, cannot be called (compile
//! error), and contributes nothing to the APK's manifest, Kotlin, Gradle
//! dependencies, or native `.so`.
//!
//! Capabilities are intentionally NOT reachable through any raw sink handle:
//! each lives only behind its own feature-gated module, so the gate cannot
//! be bypassed.
//!
//! The crate root re-exports each module (e.g. `mobile_sentinel::camera`)
//! so consumers use a short path; the grouping under `features/` is an
//! internal organisation detail.

// Shared helpers for capability modules. Always compiled — it only defines
// the `android_or!` macro (cheap, no deps), and gating it behind a feature
// list is fragile as more capabilities are added.
pub mod common;

/// Accessibility-service grant flow (ultra-protection). Gated behind `accessibility`.
#[cfg(feature = "accessibility")]
pub mod accessibility;
/// Non-firing audio preview. Gated behind `audio`.
#[cfg(feature = "audio")]
pub mod audio;
/// Battery-optimization exemption. Gated behind `battery`.
#[cfg(feature = "battery")]
pub mod battery;
/// Biometric authentication. Gated behind `biometric`.
#[cfg(feature = "biometric")]
pub mod biometric;
/// Calendar events. Gated behind `calendar`.
#[cfg(feature = "calendar")]
pub mod calendar;
/// Photo capture via the system camera app. Gated behind `camera`.
#[cfg(feature = "camera")]
pub mod camera;
/// Clipboard. Gated behind `clipboard`.
#[cfg(feature = "clipboard")]
pub mod clipboard;
/// Read device contacts. Gated behind `contacts`.
#[cfg(feature = "contacts")]
pub mod contacts;
/// Device-admin force-lock. Gated behind `device_admin`.
#[cfg(feature = "device_admin")]
pub mod device_admin;
/// Dismiss-guard (block back/swipe). Gated behind `dismiss_guard`.
#[cfg(feature = "dismiss_guard")]
pub mod dismiss_guard;
/// Display brightness + keep-screen-on. Gated behind `display`.
#[cfg(feature = "display")]
pub mod display;
/// Bundled-asset file-system helpers. Gated behind `file_system`.
#[cfg(feature = "file_system")]
pub mod file_system;
/// Foregrounding (finish activity). Gated behind `foregrounding`.
#[cfg(feature = "foregrounding")]
pub mod foregrounding;
/// Vibration / haptics. Gated behind `haptics`.
#[cfg(feature = "haptics")]
pub mod haptics;
/// Current device location. Gated behind `location`.
#[cfg(feature = "location")]
pub mod location;
/// Geocoding. Gated behind `maps`.
#[cfg(feature = "maps")]
pub mod maps;
/// Native media/file picker. Gated behind `media_picker`.
#[cfg(feature = "media_picker")]
pub mod media_picker;
/// Network connectivity status. Gated behind `network`.
#[cfg(feature = "network")]
pub mod network;
/// General notifications (post/update/cancel). Gated behind `notifications`.
#[cfg(feature = "notifications")]
pub mod notifications;
/// Draw-over-apps overlay permission. Gated behind `overlay`.
#[cfg(feature = "overlay")]
pub mod overlay;
/// Runtime permissions + app-settings deep link. Gated behind `permissions`.
#[cfg(feature = "permissions")]
pub mod permissions;
/// Phone dial / call state. Gated behind `phone`.
#[cfg(feature = "phone")]
pub mod phone;
/// Barcode / QR scanning. Gated behind `scanner`.
#[cfg(feature = "scanner")]
pub mod scanner;
/// System screen-pinning. Gated behind `screen_pin`.
#[cfg(feature = "screen_pin")]
pub mod screen_pin;
/// Encrypted key/value storage. Gated behind `secure_storage`.
#[cfg(feature = "secure_storage")]
pub mod secure_storage;
/// Accelerometer shake counter + step counter. Gated behind `sensors`.
#[cfg(feature = "sensors")]
pub mod sensors;
/// System share sheet. Gated behind `share`.
#[cfg(feature = "share")]
pub mod share;
/// SMS sending. Gated behind `sms`.
#[cfg(feature = "sms")]
pub mod sms;
/// Torch / flashlight. Gated behind `torch`.
#[cfg(feature = "torch")]
pub mod torch;

// ---- Standalone building-block features (real independent value) --------

/// Cross-process Job Guardian persistence. Gated behind `jobs`.
#[cfg(feature = "jobs")]
pub mod jobs;
/// Sound Library (bundled/custom/system resolution). Gated behind `sound-library`.
#[cfg(feature = "sound-library")]
pub mod sound;
/// Generic durable per-instance state store. Gated behind `state-store`.
#[cfg(feature = "state-store")]
pub mod state_store;

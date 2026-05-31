//! Android platform implementation using JNI.
//! This module provides:
//! - `callbacks` — Register Rust closures for Android system events (PUBLIC:
//!   the one consumer-facing surface here, reached via `mobile_sentinel::callbacks`).
//! - `context` — Activity/Context access via ndk-context (internal).
//! - `jni` — shared JNI plumbing (attach thread, resolve+cache a
//!   `com.mobilesentinel.*` class, run a closure). Holds NO per-capability
//!   logic; each capability's JNI calls live in its `crate::features::<cap>`
//!   module. `pub(crate)`.
//! - `sound_backend_android` / `firing_sink_android` — Android impls (the
//!   public types `AndroidSoundBackend` / `AndroidFiringSink` are re-exported).
//! - `sensor_state` — Global sensor state (internal).
//!
//! There is no raw JNI facade a consumer can reach: a capability is reachable
//! ONLY through its feature-gated `mobile_sentinel::<cap>` door.

pub mod callbacks;
pub(crate) mod context;
#[cfg(all(target_os = "android", firing_enabled))]
pub(crate) mod firing_sink_android;
#[cfg(target_os = "android")]
pub(crate) mod jni;
#[cfg(feature = "sensors")]
pub(crate) mod sensor_state;
#[cfg(all(target_os = "android", feature = "sound-library"))]
pub(crate) mod sound_backend_android;

#[cfg(all(target_os = "android", firing_enabled))]
pub use firing_sink_android::AndroidFiringSink;
#[cfg(all(target_os = "android", feature = "sound-library"))]
pub use sound_backend_android::AndroidSoundBackend;

/// Whether the activity whose class matches `fqcn` is the currently-resumed
/// activity, per the core `SentinelActivityTracker`. Capability-agnostic core
/// query (no alarm/kiosk semantics) — used by the firing sink to decide
/// whether a full-screen intent is redundant because the UI is already up.
/// Returns `false` on host builds / when the tracker is unavailable.
#[cfg(all(target_os = "android", firing_enabled))]
pub(crate) fn is_activity_resumed(fqcn: &str) -> bool {
    use self::jni::{jni_str, with_jni_class};
    use ::jni::objects::JValue;

    const ACTIVITY_TRACKER: &str = "com/mobilesentinel/SentinelActivityTracker";
    with_jni_class(ACTIVITY_TRACKER, false, |env, class| {
        let s = jni_str(env, fqcn)?;
        env.call_static_method(
            class,
            "isActivityResumed",
            "(Ljava/lang/String;)Z",
            &[JValue::Object(&s.into())],
        )
        .ok()
        .and_then(|v| v.z().ok())
    })
}

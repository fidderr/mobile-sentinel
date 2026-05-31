//! Shared helpers for feature-gated capability modules.
//!
//! Capability modules (`camera`, `haptics`, `audio`, …) each expose a thin
//! public API that, on Android, calls a JNI free function and, on host,
//! returns a uniform "unavailable" result. This module centralises the
//! repetitive `#[cfg(target_os = "android")]` scaffolding and the
//! host-fallback error construction so each capability module stays a small,
//! readable list of entry points.
//!
//! The [`crate::error::SentinelError::unavailable`] constructor itself lives
//! in core `error.rs` (it is a core type); this module only provides the
//! *calling pattern* helpers that capability modules share.

/// Run `android` on Android, or evaluate to `host` on every other target.
///
/// Keeps capability modules free of repeated `#[cfg]` blocks for the common
/// "do the JNI call, else return a fallback value" shape:
///
/// ```ignore
/// pub fn is_available() -> bool {
///     android_or!(android::has_torch(), false)
/// }
/// ```
#[macro_export]
#[doc(hidden)]
macro_rules! android_or {
    ($android:expr, $host:expr) => {{
        #[cfg(target_os = "android")]
        {
            $android
        }
        #[cfg(not(target_os = "android"))]
        {
            $host
        }
    }};
}

//! Haptics capability — vibration.
//!
//! Gated behind the `haptics` Cargo feature. Calling any function here
//! without enabling `haptics` is a compile error. The sole entry point is
//! this module, so the gate cannot be bypassed. The JNI calls into
//! `com.mobilesentinel.SentinelHapticPrimitives` live here.
//!
//! ```toml
//! mobile-sentinel = { version = "...", features = ["haptics"] }
//! ```

use std::time::Duration;

/// Vibrate for `duration`. No-op on host / when no vibrator is present.
pub fn vibrate(duration: Duration) {
    #[cfg(target_os = "android")]
    {
        let _ = android::vibrate(duration.as_millis() as u64);
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = duration;
    }
}

/// Play a vibration pattern of alternating wait/vibrate milliseconds.
pub fn vibrate_pattern(pattern: &[u64]) {
    #[cfg(target_os = "android")]
    {
        let _ = android::vibrate_pattern(pattern);
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = pattern;
    }
}

/// Cancel any ongoing vibration.
pub fn cancel() {
    #[cfg(target_os = "android")]
    {
        let _ = android::cancel_vibration();
    }
}

/// Whether the device has a vibrator.
pub fn has_vibrator() -> bool {
    #[cfg(target_os = "android")]
    {
        android::has_vibrator()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;
    use jni::objects::JValue;
    use jni::sys::jlong;

    const HAPTIC: &str = "com/mobilesentinel/SentinelHapticPrimitives";

    pub(super) fn vibrate(duration_ms: u64) -> bool {
        with_jni_class(HAPTIC, false, |env, class| {
            env.call_static_method(
                class,
                "vibrate",
                "(J)Z",
                &[JValue::Long(duration_ms as jlong)],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn vibrate_pattern(pattern: &[u64]) -> bool {
        with_jni_class(HAPTIC, false, |env, class| {
            let arr = env.new_long_array(pattern.len() as i32).ok()?;
            let longs: Vec<jlong> = pattern.iter().map(|&v| v as jlong).collect();
            env.set_long_array_region(&arr, 0, &longs).ok()?;
            env.call_static_method(
                class,
                "vibratePattern",
                "([J)Z",
                &[JValue::Object(&arr.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn cancel_vibration() -> bool {
        with_jni_class(HAPTIC, false, |env, class| {
            env.call_static_method(class, "cancel", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn has_vibrator() -> bool {
        with_jni_class(HAPTIC, false, |env, class| {
            env.call_static_method(class, "hasVibrator", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}

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

/// Play a vibration waveform with a strength per segment: `timings` are alternating
/// wait/vibrate milliseconds, `amplitudes` (0 = off, 1..=255 = strength) has one value per
/// timing. On a vibrator without amplitude control the timings play at fixed strength, where
/// any non-zero amplitude counts as on. Mismatched lengths are cut to the shorter one.
pub fn vibrate_waveform(timings: &[u64], amplitudes: &[u8]) {
    let n = timings.len().min(amplitudes.len());
    #[cfg(target_os = "android")]
    {
        if n > 0 {
            let _ = android::vibrate_waveform(&timings[..n], &amplitudes[..n]);
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = n;
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
    use jni::sys::{jint, jlong};

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

    pub(super) fn vibrate_waveform(timings: &[u64], amplitudes: &[u8]) -> bool {
        with_jni_class(HAPTIC, false, |env, class| {
            let times = env.new_long_array(timings.len() as i32).ok()?;
            let longs: Vec<jlong> = timings.iter().map(|&v| v as jlong).collect();
            env.set_long_array_region(&times, 0, &longs).ok()?;
            let amps = env.new_int_array(amplitudes.len() as i32).ok()?;
            let ints: Vec<jint> = amplitudes.iter().map(|&v| jint::from(v)).collect();
            env.set_int_array_region(&amps, 0, &ints).ok()?;
            env.call_static_method(
                class,
                "vibrateWaveform",
                "([J[I)Z",
                &[JValue::Object(&times.into()), JValue::Object(&amps.into())],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_is_a_no_op_on_host_for_any_lengths() {
        vibrate_waveform(&[0, 70, 80], &[0, 170]);
        vibrate_waveform(&[], &[]);
        vibrate_waveform(&[0, 30], &[0, 70, 255]);
    }
}

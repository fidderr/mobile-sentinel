//! Display / brightness capability. Gated behind `display`.
//!
//! Screen brightness control + keep-screen-on. Sole entry point; not
//! reachable through any raw sink handle. The JNI calls into
//! `com.mobilesentinel.SentinelDisplayPrimitives` live here.

use crate::android_or;

/// Set screen brightness (0.0–1.0).
pub fn set_brightness(level: f32) {
    android_or!(
        {
            let _ = android::set_brightness(level);
        },
        {
            let _ = level;
        }
    )
}

/// Current screen brightness (0.0–1.0). Returns 0.0 on host; on Android a
/// negative value (-1.0) means "unknown / system default" per the platform.
pub fn brightness() -> f32 {
    android_or!(android::get_brightness(), 0.0)
}

/// Force maximum brightness.
pub fn set_max_brightness() {
    android_or!(
        {
            let _ = android::set_max_brightness();
        },
        ()
    )
}

/// Restore brightness to the pre-modification value.
pub fn restore_brightness() {
    android_or!(
        {
            let _ = android::restore_brightness();
        },
        ()
    )
}

/// Keep the screen on (or release).
pub fn keep_screen_on(enabled: bool) {
    android_or!(
        {
            let _ = android::keep_screen_on(enabled);
        },
        {
            let _ = enabled;
        }
    )
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;
    use jni::objects::JValue;

    const DISPLAY: &str = "com/mobilesentinel/SentinelDisplayPrimitives";

    pub(super) fn set_brightness(level: f32) -> bool {
        with_jni_class(DISPLAY, false, |env, class| {
            env.call_static_method(class, "setBrightness", "(F)Z", &[JValue::Float(level)])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn get_brightness() -> f32 {
        with_jni_class(DISPLAY, -1.0f32, |env, class| {
            env.call_static_method(class, "getBrightness", "()F", &[])
                .ok()
                .and_then(|v| v.f().ok())
        })
    }

    pub(super) fn set_max_brightness() -> bool {
        with_jni_class(DISPLAY, false, |env, class| {
            env.call_static_method(class, "setMaxBrightness", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn restore_brightness() -> bool {
        with_jni_class(DISPLAY, false, |env, class| {
            env.call_static_method(class, "restoreBrightness", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn keep_screen_on(enabled: bool) -> bool {
        with_jni_class(DISPLAY, false, |env, class| {
            env.call_static_method(
                class,
                "keepScreenOn",
                "(Z)Z",
                &[JValue::Bool(enabled as u8)],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }
}

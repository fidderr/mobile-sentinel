//! Battery-optimization capability. Gated behind `battery`.
//!
//! Query / request exemption from battery optimization (improves alarm
//! reliability on aggressive OEMs). Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelBatteryPrimitives` live here.

use crate::android_or;

/// Whether the app is exempt from battery optimization.
pub fn is_exempt() -> bool {
    android_or!(android::battery_is_exempt(), false)
}

/// Request battery-optimization exemption (shows the system prompt).
pub fn request_exemption() {
    android_or!(
        {
            let _ = android::request_battery_exemption();
        },
        ()
    )
}

/// Open the battery-optimization settings page.
pub fn open_settings() {
    android_or!(
        {
            let _ = android::open_battery_settings();
        },
        ()
    )
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;

    const BATTERY: &str = "com/mobilesentinel/SentinelBatteryPrimitives";

    pub(super) fn battery_is_exempt() -> bool {
        with_jni_class(BATTERY, false, |env, class| {
            env.call_static_method(class, "isExempt", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn request_battery_exemption() -> bool {
        with_jni_class(BATTERY, false, |env, class| {
            env.call_static_method(class, "requestExemption", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn open_battery_settings() -> bool {
        with_jni_class(BATTERY, false, |env, class| {
            env.call_static_method(class, "openSettings", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}

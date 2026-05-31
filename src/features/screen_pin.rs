//! Screen-pinning capability (system lock-task pinning). Gated behind
//! `screen_pin`. Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelScreenPinPrimitives` live here.

use crate::android_or;

/// Pin the current screen (system screen-pinning).
pub fn pin() {
    android_or!(
        {
            let _ = android::pin_screen();
        },
        ()
    )
}

/// Unpin the screen.
pub fn unpin() {
    android_or!(
        {
            let _ = android::unpin_screen();
        },
        ()
    )
}

/// Whether the screen is currently pinned.
pub fn is_pinned() -> bool {
    android_or!(android::is_screen_pinned(), false)
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;

    const SCREEN_PIN: &str = "com/mobilesentinel/SentinelScreenPinPrimitives";

    pub(super) fn pin_screen() -> bool {
        with_jni_class(SCREEN_PIN, false, |env, class| {
            env.call_static_method(class, "pin", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn unpin_screen() -> bool {
        with_jni_class(SCREEN_PIN, false, |env, class| {
            env.call_static_method(class, "unpin", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn is_screen_pinned() -> bool {
        with_jni_class(SCREEN_PIN, false, |env, class| {
            env.call_static_method(class, "isPinned", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}

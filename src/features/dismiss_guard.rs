//! Dismiss-guard capability — block back/swipe dismissal. Gated behind
//! `dismiss_guard`. Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelDismissGuardPrimitives` live here.

use crate::android_or;

/// Activate dismiss prevention (block back / recents-swipe).
pub fn activate() {
    android_or!(
        {
            let _ = android::dismiss_guard_activate();
        },
        ()
    )
}

/// Deactivate dismiss prevention.
pub fn deactivate() {
    android_or!(
        {
            let _ = android::dismiss_guard_deactivate();
        },
        ()
    )
}

/// Whether dismiss prevention is active.
pub fn is_active() -> bool {
    android_or!(android::dismiss_guard_active(), false)
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;

    const DISMISS_GUARD: &str = "com/mobilesentinel/SentinelDismissGuardPrimitives";

    pub(super) fn dismiss_guard_activate() -> bool {
        with_jni_class(DISMISS_GUARD, false, |env, class| {
            env.call_static_method(class, "activate", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn dismiss_guard_deactivate() -> bool {
        with_jni_class(DISMISS_GUARD, false, |env, class| {
            env.call_static_method(class, "deactivate", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn dismiss_guard_active() -> bool {
        with_jni_class(DISMISS_GUARD, false, |env, class| {
            env.call_static_method(class, "isActive", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}

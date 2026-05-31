//! Overlay capability — "draw over other apps" (SYSTEM_ALERT_WINDOW).
//!
//! Gated behind the `overlay` Cargo feature. Lets the consumer check /
//! request the overlay permission used to force a firing UI to the
//! foreground while the screen is on and unlocked.
//!
//! The sole entry point is this module. The JNI call into
//! `com.mobilesentinel.SentinelOverlayPrimitives` lives here.
//!
//! ```toml
//! mobile-sentinel = { version = "...", features = ["overlay"] }
//! ```

/// Whether the "draw over other apps" overlay permission is granted.
pub fn is_granted() -> bool {
    #[cfg(target_os = "android")]
    {
        android::is_granted()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Launch the overlay-permission settings screen.
pub fn request() {
    #[cfg(target_os = "android")]
    {
        let _ = android::request_overlay_permission();
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;

    const OVERLAY: &str = "com/mobilesentinel/SentinelOverlayPrimitives";

    pub(super) fn is_granted() -> bool {
        with_jni_class(OVERLAY, false, |env, class| {
            env.call_static_method(class, "isGranted", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn request_overlay_permission() -> bool {
        with_jni_class(OVERLAY, false, |env, class| {
            env.call_static_method(class, "request", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}

//! Device-admin capability — force-lock privileges. Gated behind
//! `device_admin` (BIND_DEVICE_ADMIN). POLICY-SENSITIVE on Google Play.
//! Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelDeviceAdminPrimitives` live here.

use crate::android_or;

/// Whether device-admin is currently active.
pub fn is_active() -> bool {
    android_or!(android::device_admin_active(), false)
}

/// Launch the system "add device admin" screen.
pub fn request() {
    android_or!(
        {
            let _ = android::request_device_admin();
        },
        ()
    )
}

/// Relinquish device-admin.
pub fn relinquish() {
    android_or!(
        {
            let _ = android::relinquish_device_admin();
        },
        ()
    )
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;

    const DEVICE_ADMIN: &str = "com/mobilesentinel/SentinelDeviceAdminPrimitives";

    pub(super) fn device_admin_active() -> bool {
        with_jni_class(DEVICE_ADMIN, false, |env, class| {
            env.call_static_method(class, "isActive", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn request_device_admin() -> bool {
        with_jni_class(DEVICE_ADMIN, false, |env, class| {
            env.call_static_method(class, "requestActivation", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn relinquish_device_admin() -> bool {
        with_jni_class(DEVICE_ADMIN, false, |env, class| {
            env.call_static_method(class, "relinquish", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}

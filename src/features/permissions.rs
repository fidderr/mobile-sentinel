//! Permissions capability — runtime permission status / request, and the
//! app-settings deep link.
//!
//! Gated behind the `permissions` Cargo feature. The sole entry point is
//! this module. The JNI calls into
//! `com.mobilesentinel.SentinelPermissionPrimitives` live here.
//!
//! ```toml
//! mobile-sentinel = { version = "...", features = ["permissions"] }
//! ```

pub use crate::sink_types::PermissionState;

/// Current status of a runtime permission (e.g.
/// `"android.permission.POST_NOTIFICATIONS"`).
pub fn status(permission: &str) -> PermissionState {
    #[cfg(target_os = "android")]
    {
        if android::check_permission(permission) {
            PermissionState::Granted
        } else {
            PermissionState::Denied
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = permission;
        PermissionState::NotDetermined
    }
}

/// Request a runtime permission. Returns the resulting state.
pub fn request(permission: &str) -> PermissionState {
    #[cfg(target_os = "android")]
    {
        // Kotlin returns 0 = granted, 1 = denied (no activity), 2 = denied.
        match android::request_runtime_permission(permission) {
            0 => PermissionState::Granted,
            _ => PermissionState::Denied,
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = permission;
        PermissionState::NotDetermined
    }
}

/// Open the app's system settings detail page so the user can re-grant a
/// permission they previously denied (e.g. notifications).
pub fn open_app_settings() {
    #[cfg(target_os = "android")]
    {
        let _ = android::open_app_settings();
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::JValue;

    const PERMISSIONS: &str = "com/mobilesentinel/SentinelPermissionPrimitives";

    pub(super) fn request_runtime_permission(permission: &str) -> i32 {
        with_jni_class(PERMISSIONS, 1, |env, class| {
            let s = jni_str(env, permission)?;
            env.call_static_method(
                class,
                "requestRuntimePermission",
                "(Ljava/lang/String;)I",
                &[JValue::Object(&s.into())],
            )
            .ok()
            .and_then(|v| v.i().ok())
        })
    }

    pub(super) fn check_permission(permission: &str) -> bool {
        with_jni_class(PERMISSIONS, false, |env, class| {
            let s = jni_str(env, permission)?;
            env.call_static_method(
                class,
                "checkPermission",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&s.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn open_app_settings() -> bool {
        with_jni_class(PERMISSIONS, false, |env, class| {
            env.call_static_method(class, "openAppSettings", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}

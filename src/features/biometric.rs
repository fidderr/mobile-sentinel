//! Biometric capability — fingerprint / face authentication. Gated behind
//! `biometric`. Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelBiometricsPrimitives` live here.

use crate::error::SentinelError;
pub use crate::sink_types::BiometricType;

/// Whether biometric auth is available.
pub fn is_available() -> bool {
    #[cfg(target_os = "android")]
    {
        android::biometrics_available()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Prompt for biometric auth with `reason`, and wait for the answer: `Ok` only if the
/// user was recognised. Blocks, so not on the UI thread. Needs the app's Activity to be a
/// `FragmentActivity` and on screen.
pub fn authenticate(reason: &str) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::biometrics_authenticate(reason) {
            return Ok(());
        }
        Err(SentinelError::unavailable("biometric::authenticate"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = reason;
        Err(SentinelError::unavailable("biometric::authenticate"))
    }
}

/// Whether this device can ask who is there at all: biometrics, or the screen lock.
pub fn can_ask_with_screen_lock() -> bool {
    #[cfg(target_os = "android")]
    {
        android::can_ask_with_screen_lock()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Like [`authenticate`], and whoever has no biometrics, or fails them, can use the
/// screen lock's PIN, pattern or password. Blocks, so not on the UI thread.
pub fn authenticate_or_screen_lock(reason: &str) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::authenticate_or_screen_lock(reason) {
            return Ok(());
        }
        Err(SentinelError::unavailable(
            "biometric::authenticate_or_screen_lock",
        ))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = reason;
        Err(SentinelError::unavailable(
            "biometric::authenticate_or_screen_lock",
        ))
    }
}

/// The biometric hardware type present.
pub fn biometric_type() -> BiometricType {
    #[cfg(target_os = "android")]
    {
        match android::biometric_type() {
            1 => BiometricType::Fingerprint,
            2 => BiometricType::Face,
            _ => BiometricType::None,
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        BiometricType::None
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::JValue;

    const BIOMETRICS: &str = "com/mobilesentinel/SentinelBiometricsPrimitives";

    pub(super) fn biometrics_available() -> bool {
        with_jni_class(BIOMETRICS, false, |env, class| {
            env.call_static_method(class, "isAvailable", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn can_ask_with_screen_lock() -> bool {
        with_jni_class(BIOMETRICS, false, |env, class| {
            env.call_static_method(class, "canAskWithScreenLock", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn authenticate_or_screen_lock(reason: &str) -> bool {
        with_jni_class(BIOMETRICS, false, |env, class| {
            let s = jni_str(env, reason)?;
            env.call_static_method(
                class,
                "authenticateOrScreenLock",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&s.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    /// Returns 0 = None, 1 = Fingerprint, 2 = Face.
    pub(super) fn biometric_type() -> i32 {
        with_jni_class(BIOMETRICS, 0, |env, class| {
            env.call_static_method(class, "biometricType", "()I", &[])
                .ok()
                .and_then(|v| v.i().ok())
        })
    }

    pub(super) fn biometrics_authenticate(reason: &str) -> bool {
        with_jni_class(BIOMETRICS, false, |env, class| {
            let s = jni_str(env, reason)?;
            env.call_static_method(
                class,
                "authenticate",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&s.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }
}

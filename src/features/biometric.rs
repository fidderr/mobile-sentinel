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

/// Prompt for biometric auth with `reason`.
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

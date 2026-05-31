//! Secure storage capability — encrypted key/value. Gated behind
//! `secure_storage`. Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelSecureStoragePrimitives` live here.

use crate::error::SentinelError;

/// Store `value` under `key`.
pub fn set(key: &str, value: &str) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::secure_set(key, value) {
            return Ok(());
        }
        Err(SentinelError::unavailable("secure_storage::set"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (key, value);
        Err(SentinelError::unavailable("secure_storage::set"))
    }
}

/// Read the value for `key`, if present.
pub fn get(key: &str) -> Result<Option<String>, SentinelError> {
    #[cfg(target_os = "android")]
    {
        Ok(android::secure_get(key))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = key;
        Err(SentinelError::unavailable("secure_storage::get"))
    }
}

/// Delete the value for `key`.
pub fn delete(key: &str) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::secure_delete(key) {
            return Ok(());
        }
        Err(SentinelError::unavailable("secure_storage::delete"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = key;
        Err(SentinelError::unavailable("secure_storage::delete"))
    }
}

/// Clear all stored values.
pub fn clear() -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::secure_clear() {
            return Ok(());
        }
        Err(SentinelError::unavailable("secure_storage::clear"))
    }
    #[cfg(not(target_os = "android"))]
    {
        Err(SentinelError::unavailable("secure_storage::clear"))
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::{JString, JValue};

    const SECURE: &str = "com/mobilesentinel/SentinelSecureStoragePrimitives";

    pub(super) fn secure_set(key: &str, value: &str) -> bool {
        with_jni_class(SECURE, false, |env, class| {
            let s_key = jni_str(env, key)?;
            let s_val = jni_str(env, value)?;
            env.call_static_method(
                class,
                "set",
                "(Ljava/lang/String;Ljava/lang/String;)Z",
                &[JValue::Object(&s_key.into()), JValue::Object(&s_val.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn secure_get(key: &str) -> Option<String> {
        with_jni_class(SECURE, None, |env, class| {
            let s_key = jni_str(env, key)?;
            let res = env
                .call_static_method(
                    class,
                    "get",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&s_key.into())],
                )
                .ok()?;
            let obj = res.l().ok()?;
            if obj.is_null() {
                return Some(None);
            }
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(Some(s))
        })
    }

    pub(super) fn secure_delete(key: &str) -> bool {
        with_jni_class(SECURE, false, |env, class| {
            let s_key = jni_str(env, key)?;
            env.call_static_method(
                class,
                "delete",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&s_key.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn secure_clear() -> bool {
        with_jni_class(SECURE, false, |env, class| {
            env.call_static_method(class, "clear", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}

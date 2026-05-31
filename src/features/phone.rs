//! Phone capability — dial / call state. Gated behind `phone` (CALL_PHONE).
//! Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelPhonePrimitives` live here.

use crate::error::SentinelError;

/// Dial `number` (opens the dialer / places the call per platform policy).
pub fn dial(number: &str) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::phone_dial(number) {
            return Ok(());
        }
        Err(SentinelError::unavailable("phone::dial"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = number;
        Err(SentinelError::unavailable("phone::dial"))
    }
}

/// Whether the device is currently in a call.
pub fn is_in_call() -> bool {
    #[cfg(target_os = "android")]
    {
        android::phone_is_in_call()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::JValue;

    const PHONE: &str = "com/mobilesentinel/SentinelPhonePrimitives";

    pub(super) fn phone_dial(number: &str) -> bool {
        with_jni_class(PHONE, false, |env, class| {
            let s_num = jni_str(env, number)?;
            env.call_static_method(
                class,
                "dial",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&s_num.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn phone_is_in_call() -> bool {
        with_jni_class(PHONE, false, |env, class| {
            env.call_static_method(class, "isInCall", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}

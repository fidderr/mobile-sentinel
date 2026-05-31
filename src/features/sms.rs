//! SMS capability — send text messages. Gated behind `sms` (SEND_SMS).
//! Sole entry point. The JNI calls into
//! `com.mobilesentinel.SentinelSmsPrimitives` live here.

use crate::error::SentinelError;

/// Send an SMS to `number`.
pub fn send(number: &str, message: &str) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::sms_send(number, message) {
            return Ok(());
        }
        Err(SentinelError::unavailable("sms::send"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (number, message);
        Err(SentinelError::unavailable("sms::send"))
    }
}

/// Whether SMS sending is available on this device.
pub fn is_available() -> bool {
    #[cfg(target_os = "android")]
    {
        android::sms_available()
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

    const SMS: &str = "com/mobilesentinel/SentinelSmsPrimitives";

    pub(super) fn sms_send(number: &str, message: &str) -> bool {
        with_jni_class(SMS, false, |env, class| {
            let s_num = jni_str(env, number)?;
            let s_msg = jni_str(env, message)?;
            env.call_static_method(
                class,
                "send",
                "(Ljava/lang/String;Ljava/lang/String;)Z",
                &[JValue::Object(&s_num.into()), JValue::Object(&s_msg.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn sms_available() -> bool {
        with_jni_class(SMS, false, |env, class| {
            env.call_static_method(class, "isAvailable", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}

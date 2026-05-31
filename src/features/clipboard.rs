//! Clipboard capability. Gated behind `clipboard`. Sole entry point. The
//! JNI calls into `com.mobilesentinel.SentinelClipboardPrimitives` live here.

use crate::error::SentinelError;

/// Set clipboard text.
pub fn set_text(text: &str) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::clipboard_set_text(text) {
            return Ok(());
        }
        Err(SentinelError::unavailable("clipboard::set_text"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = text;
        Err(SentinelError::unavailable("clipboard::set_text"))
    }
}

/// Get clipboard text, if any.
pub fn get_text() -> Result<Option<String>, SentinelError> {
    #[cfg(target_os = "android")]
    {
        Ok(android::clipboard_get_text())
    }
    #[cfg(not(target_os = "android"))]
    {
        Err(SentinelError::unavailable("clipboard::get_text"))
    }
}

/// Whether the clipboard currently holds text.
pub fn has_text() -> bool {
    #[cfg(target_os = "android")]
    {
        android::clipboard_has_text()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::{JString, JValue};

    const CLIPBOARD: &str = "com/mobilesentinel/SentinelClipboardPrimitives";

    pub(super) fn clipboard_set_text(text: &str) -> bool {
        with_jni_class(CLIPBOARD, false, |env, class| {
            let s = jni_str(env, text)?;
            env.call_static_method(
                class,
                "setText",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&s.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn clipboard_get_text() -> Option<String> {
        with_jni_class(CLIPBOARD, None, |env, class| {
            let res = env
                .call_static_method(class, "getText", "()Ljava/lang/String;", &[])
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

    pub(super) fn clipboard_has_text() -> bool {
        with_jni_class(CLIPBOARD, false, |env, class| {
            env.call_static_method(class, "hasText", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}

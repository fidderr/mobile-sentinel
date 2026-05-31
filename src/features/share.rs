//! Share capability — system share sheet. Gated behind `share`. Sole entry
//! point. The JNI calls into `com.mobilesentinel.SentinelSharePrimitives`
//! live here.

use crate::error::SentinelError;

/// Share plain text.
pub fn text(text: &str, title: Option<&str>) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::share_text(text, title) {
            return Ok(());
        }
        Err(SentinelError::unavailable("share::text"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (text, title);
        Err(SentinelError::unavailable("share::text"))
    }
}

/// Share a URL.
pub fn url(url: &str, title: Option<&str>) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::share_url(url, title) {
            return Ok(());
        }
        Err(SentinelError::unavailable("share::url"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (url, title);
        Err(SentinelError::unavailable("share::url"))
    }
}

/// Share a file by path + MIME type.
pub fn file(path: &str, mime_type: &str) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::share_file(path, mime_type) {
            return Ok(());
        }
        Err(SentinelError::unavailable("share::file"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (path, mime_type);
        Err(SentinelError::unavailable("share::file"))
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::{JObject, JValue};
    use jni::JNIEnv;

    const SHARE: &str = "com/mobilesentinel/SentinelSharePrimitives";

    /// Build a nullable Java String from `Option<&str>`: `None` → JNI null.
    fn opt_jstr<'a>(env: &mut JNIEnv<'a>, s: Option<&str>) -> Option<JObject<'a>> {
        match s {
            Some(v) => Some(jni_str(env, v)?.into()),
            None => Some(JObject::null()),
        }
    }

    pub(super) fn share_text(text: &str, title: Option<&str>) -> bool {
        with_jni_class(SHARE, false, |env, class| {
            let s_text = jni_str(env, text)?;
            let s_title = opt_jstr(env, title)?;
            env.call_static_method(
                class,
                "shareText",
                "(Ljava/lang/String;Ljava/lang/String;)Z",
                &[JValue::Object(&s_text.into()), JValue::Object(&s_title)],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn share_url(url: &str, title: Option<&str>) -> bool {
        with_jni_class(SHARE, false, |env, class| {
            let s_url = jni_str(env, url)?;
            let s_title = opt_jstr(env, title)?;
            env.call_static_method(
                class,
                "shareUrl",
                "(Ljava/lang/String;Ljava/lang/String;)Z",
                &[JValue::Object(&s_url.into()), JValue::Object(&s_title)],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn share_file(path: &str, mime_type: &str) -> bool {
        with_jni_class(SHARE, false, |env, class| {
            let s_path = jni_str(env, path)?;
            let s_mime = jni_str(env, mime_type)?;
            env.call_static_method(
                class,
                "shareFile",
                "(Ljava/lang/String;Ljava/lang/String;)Z",
                &[
                    JValue::Object(&s_path.into()),
                    JValue::Object(&s_mime.into()),
                ],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }
}

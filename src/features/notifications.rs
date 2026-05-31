//! Notifications capability — post / update / cancel. Gated behind
//! `notifications`. Sole entry point.
//!
//! (Distinct from the firing FGS notification, which the firing surface
//! manages internally.) The JNI calls into
//! `com.mobilesentinel.SentinelNotificationPrimitives` live here.

/// Post a notification. Returns `true` on success.
pub fn post(
    id: &str,
    channel_id: &str,
    title: &str,
    body: &str,
    importance: i32,
    full_screen_intent: bool,
) -> bool {
    #[cfg(target_os = "android")]
    {
        android::post_notification(id, channel_id, title, body, importance, full_screen_intent)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (id, channel_id, title, body, importance, full_screen_intent);
        false
    }
}

/// Update an existing notification's title/body.
pub fn update(id: &str, title: &str, body: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        android::update_notification(id, title, body)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (id, title, body);
        false
    }
}

/// Cancel a posted notification.
pub fn cancel(id: &str) {
    #[cfg(target_os = "android")]
    {
        let _ = android::cancel_notification(id);
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = id;
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::JValue;
    use jni::sys::{jboolean, jint};

    const NOTIFICATIONS: &str = "com/mobilesentinel/SentinelNotificationPrimitives";

    pub(super) fn post_notification(
        id: &str,
        channel_id: &str,
        title: &str,
        body: &str,
        importance: i32,
        full_screen_intent: bool,
    ) -> bool {
        with_jni_class(NOTIFICATIONS, false, |env, class| {
            let s_id = jni_str(env, id)?;
            let s_ch = jni_str(env, channel_id)?;
            let s_title = jni_str(env, title)?;
            let s_body = jni_str(env, body)?;
            env.call_static_method(
                class,
                "postNotification",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;IZ)Z",
                &[
                    JValue::Object(&s_id.into()),
                    JValue::Object(&s_ch.into()),
                    JValue::Object(&s_title.into()),
                    JValue::Object(&s_body.into()),
                    JValue::Int(importance as jint),
                    JValue::Bool(full_screen_intent as jboolean),
                ],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn cancel_notification(id: &str) -> bool {
        with_jni_class(NOTIFICATIONS, false, |env, class| {
            let s = jni_str(env, id)?;
            env.call_static_method(
                class,
                "cancelNotification",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&s.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn update_notification(id: &str, title: &str, body: &str) -> bool {
        with_jni_class(NOTIFICATIONS, false, |env, class| {
            let s_id = jni_str(env, id)?;
            let s_title = jni_str(env, title)?;
            let s_body = jni_str(env, body)?;
            env.call_static_method(
                class,
                "updateNotification",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Z",
                &[
                    JValue::Object(&s_id.into()),
                    JValue::Object(&s_title.into()),
                    JValue::Object(&s_body.into()),
                ],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }
}

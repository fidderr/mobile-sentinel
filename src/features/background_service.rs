//! Background service capability. Gated behind `background_service`.
//!
//! Keeps the app's process alive after its UI is closed so a messenger can
//! hold its connection and get messages at once, like any chat app: a
//! foreground service with the quietest notification Android allows, a boot
//! receiver, message/call notifications whose tap opens the right screen, and
//! a Back-button hook that sends the app to the background instead of
//! destroying it.
//!
//! The host app implements two JNI hooks (both optional):
//!
//! * `Java_com_mobilesentinel_SentinelBackground_nativeStart(env, class, data_dir)`:
//!   the service calls it when the process was started just for the service
//!   (no activity), so the app can bring its core up. Call [`warm_up`] from it.
//! * `Java_com_mobilesentinel_SentinelBackground_nativeOnBack(env, class)`:
//!   Back was pressed; answer by going back a screen or by calling
//!   [`move_to_back`].
//!
//! Battery-optimisation exemption and the notification permission use the
//! `battery` and `permissions` capabilities.

use crate::android_or;

/// Which notification channel (and so sound/priority) a notification uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Message,
    /// An incoming call: rings, and may take over the screen.
    Call,
    Request,
}

impl Kind {
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    fn code(self) -> i32 {
        match self {
            Kind::Message => 0,
            Kind::Call => 1,
            Kind::Request => 2,
        }
    }
}

/// Sets the (translated) texts: the service notification's title and body, and the
/// channel names shown in the system's notification settings.
pub fn set_texts(
    service_title: &str,
    service_text: &str,
    messages: &str,
    calls: &str,
    requests: &str,
) {
    android_or!(
        android::set_texts(service_title, service_text, messages, calls, requests),
        {
            let _ = (service_title, service_text, messages, calls, requests);
        }
    )
}

/// Starts the service and remembers that it should run after a reboot.
pub fn start() -> bool {
    android_or!(android::start(), false)
}

/// Stops the service and forgets it (no restart after a reboot).
pub fn stop() {
    android_or!(android::stop(), ())
}

/// Whether the service is meant to be running.
pub fn is_enabled() -> bool {
    android_or!(android::is_enabled(), false)
}

/// Posts (or replaces, by `tag`) a notification. `open` comes back from
/// [`take_open_request`] when the user taps it.
pub fn post(tag: &str, title: &str, body: &str, open: &str, kind: Kind) -> bool {
    android_or!(android::post(tag, title, body, open, kind.code()), {
        let _ = (tag, title, body, open, kind);
        false
    })
}

pub fn cancel(tag: &str) {
    android_or!(android::cancel(tag), {
        let _ = tag;
    })
}

/// Removes every notification this capability posted.
pub fn cancel_all() {
    android_or!(android::cancel_all(), ())
}

/// What a tapped notification asked the app to open (its `open` string), once.
pub fn take_open_request() -> Option<String> {
    android_or!(android::take_open_request(), None)
}

/// True while one of the app's activities is in front.
pub fn is_foreground() -> bool {
    android_or!(android::is_foreground(), false)
}

/// Sends the app to the background (like Home), keeping its activity alive.
pub fn move_to_back() -> bool {
    android_or!(android::move_to_back(), false)
}

/// Call once from the app's `nativeStart` hook, on the thread the service
/// called it on: resolves the helper classes there, where the app's class
/// loader is available, so other threads can use them later.
pub fn warm_up() {
    android_or!(android::warm_up(), ())
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::{JString, JValue};
    use jni::sys::jint;

    const BG: &str = "com/mobilesentinel/SentinelBackground";

    pub(super) fn warm_up() {
        with_jni_class(BG, (), |_env, _class| Some(()));
    }

    pub(super) fn set_texts(a: &str, b: &str, c: &str, d: &str, e: &str) {
        with_jni_class(BG, (), |env, class| {
            let strs = [a, b, c, d, e]
                .iter()
                .map(|s| jni_str(env, s))
                .collect::<Option<Vec<_>>>()?;
            let args: Vec<JValue> = strs.iter().map(|s| JValue::Object(s)).collect();
            env.call_static_method(
                class,
                "setTexts",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
                &args,
            )
            .ok()
            .map(|_| ())
        })
    }

    fn call_bool(name: &str) -> bool {
        with_jni_class(BG, false, |env, class| {
            env.call_static_method(class, name, "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    fn call_unit(name: &str) {
        with_jni_class(BG, (), |env, class| {
            env.call_static_method(class, name, "()V", &[])
                .ok()
                .map(|_| ())
        })
    }

    pub(super) fn start() -> bool {
        call_bool("start")
    }

    pub(super) fn stop() {
        call_unit("stop")
    }

    pub(super) fn is_enabled() -> bool {
        call_bool("isEnabled")
    }

    pub(super) fn is_foreground() -> bool {
        call_bool("isForeground")
    }

    pub(super) fn move_to_back() -> bool {
        call_bool("moveToBack")
    }

    pub(super) fn cancel_all() {
        call_unit("cancelAll")
    }

    pub(super) fn cancel(tag: &str) {
        with_jni_class(BG, (), |env, class| {
            let t = jni_str(env, tag)?;
            env.call_static_method(
                class,
                "cancel",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&t.into())],
            )
            .ok()
            .map(|_| ())
        })
    }

    pub(super) fn post(tag: &str, title: &str, body: &str, open: &str, kind: i32) -> bool {
        with_jni_class(BG, false, |env, class| {
            let (t, ti, b, o) = (
                jni_str(env, tag)?,
                jni_str(env, title)?,
                jni_str(env, body)?,
                jni_str(env, open)?,
            );
            env.call_static_method(
                class,
                "post",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;I)Z",
                &[
                    JValue::Object(&t.into()),
                    JValue::Object(&ti.into()),
                    JValue::Object(&b.into()),
                    JValue::Object(&o.into()),
                    JValue::Int(kind as jint),
                ],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn take_open_request() -> Option<String> {
        with_jni_class(BG, None, |env, class| {
            let obj = env
                .call_static_method(class, "takeOpenRequest", "()Ljava/lang/String;", &[])
                .ok()?
                .l()
                .ok()?;
            if obj.is_null() {
                return Some(None);
            }
            let s: JString = obj.into();
            Some(env.get_string(&s).ok().map(Into::into))
        })
    }
}

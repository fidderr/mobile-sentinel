//! Home-screen widget capability: two lines of text the app sets, and tapping it opens
//! the app.
//!
//! Gated behind the `home_widget` Cargo feature. The JNI calls into
//! `com.mobilesentinel.SentinelHomeWidgetPrimitives` live here.
//!
//! ```toml
//! mobile-sentinel = { version = "...", features = ["home_widget"] }
//! ```

/// Set what the widget says. It keeps this when the app is not running, so it cannot say
/// anything that stops being true by itself ("tomorrow"). Cheap when nothing changed.
pub fn set_text(title: &str, detail: &str) {
    #[cfg(target_os = "android")]
    {
        let _ = android::set_text(title, detail);
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (title, detail);
    }
}

/// Whether the launcher can be asked to place the widget. Android 8 and newer, if the
/// launcher goes along with it.
pub fn can_add() -> bool {
    #[cfg(target_os = "android")]
    {
        android::can_add()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Ask the launcher to place the widget. It asks the user first. Whether it got as far
/// as asking.
pub fn add() -> bool {
    #[cfg(target_os = "android")]
    {
        android::add()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;
    use jni::objects::JValue;

    const WIDGET: &str = "com/mobilesentinel/SentinelHomeWidgetPrimitives";

    pub(super) fn set_text(title: &str, detail: &str) -> bool {
        with_jni_class(WIDGET, false, |env, class| {
            let title = env.new_string(title).ok()?;
            let detail = env.new_string(detail).ok()?;
            env.call_static_method(
                class,
                "setText",
                "(Ljava/lang/String;Ljava/lang/String;)Z",
                &[JValue::Object(&title), JValue::Object(&detail)],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn can_add() -> bool {
        with_jni_class(WIDGET, false, |env, class| {
            env.call_static_method(class, "canAdd", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn add() -> bool {
        with_jni_class(WIDGET, false, |env, class| {
            env.call_static_method(class, "add", "()Z", &[])
                .ok()
                .and_then(|v| v.z().ok())
        })
    }
}

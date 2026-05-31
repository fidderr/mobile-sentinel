//! Media picker capability — native file/image/video picker. Gated behind
//! `media_picker`. Sole entry point; not a trait method.
//!
//! Use this instead of an HTML `<input type="file">` in a WebView — the
//! WebView chooser dialog freezes the Dioxus Android WebView. The JNI call
//! into `com.mobilesentinel.SentinelFilePickerHelper` lives here.

use crate::error::SentinelError;

/// Pick a file matching any of `mime_types` (e.g. `["audio/*"]`); returns
/// the chosen file's path. Blocks until the user picks or cancels.
pub fn pick_file(mime_types: &[&str]) -> Result<String, SentinelError> {
    #[cfg(target_os = "android")]
    {
        let mime = mime_types.first().copied().unwrap_or("*/*");
        let path = android::pick_file_blocking(mime);
        if path.is_empty() {
            Err(SentinelError::unavailable("media_picker::pick_file"))
        } else {
            Ok(path)
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = mime_types;
        Err(SentinelError::unavailable("media_picker::pick_file"))
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::context::get_activity;
    use crate::platform::android::jni::resolve_helper_class;
    use jni::objects::{JClass, JString, JValue};

    const FILE_PICKER: &str = "com/mobilesentinel/SentinelFilePickerHelper";

    /// Blocking file pick. Returns the chosen file's internal path (copied
    /// into the app's import dir), or empty on cancel/timeout. Resolves the
    /// current Activity via `ndk_context` (the helper's `pickFileBlocking`
    /// requires an Activity to launch the document picker).
    pub(super) fn pick_file_blocking(mime: &str) -> String {
        let (vm, activity) = match get_activity() {
            Ok(pair) => pair,
            Err(_) => {
                log::warn!("[media_picker] pick_file_blocking: no activity");
                return String::new();
            }
        };
        let mut env = match vm.attach_current_thread_permanently() {
            Ok(e) => e,
            Err(e) => {
                log::warn!("[media_picker] pick_file_blocking attach failed: {e:?}");
                return String::new();
            }
        };
        let class = match resolve_helper_class(&mut env, FILE_PICKER) {
            Some(c) => c,
            None => return String::new(),
        };
        let class_ref: &JClass = <&JClass>::from(class.as_obj());
        let s_mime = match env.new_string(mime) {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
        let res = env.call_static_method(
            class_ref,
            "pickFileBlocking",
            "(Landroid/app/Activity;Ljava/lang/String;J)Ljava/lang/String;",
            &[
                JValue::Object(activity.as_obj()),
                JValue::Object(&s_mime.into()),
                JValue::Long(120),
            ],
        );
        match res.ok().and_then(|v| v.l().ok()) {
            Some(obj) if !obj.is_null() => {
                let jstr: JString = obj.into();
                env.get_string(&jstr).map(|s| s.into()).unwrap_or_default()
            }
            _ => String::new(),
        }
    }
}

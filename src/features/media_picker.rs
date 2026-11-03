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

/// A file the user picked, with the metadata a messenger or uploader needs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PickedFile {
    /// Absolute path of a readable copy (Android: inside the app's import
    /// dir; desktop: the original file).
    pub path: String,
    /// The file's original display name, e.g. `holiday.jpg`.
    pub name: String,
    /// MIME type if known, else `application/octet-stream`.
    pub mime: String,
    /// Size in bytes.
    pub size: u64,
}

/// Pick one or more files matching any of `mime_types` (e.g.
/// `["image/*", "video/*"]`; empty = any file). Blocks until the user picks
/// or cancels; a cancel returns an empty list.
///
/// Unlike [`pick_file`] this keeps each file's original name and MIME type,
/// supports multi-select, and also works on Windows (native file dialog).
pub fn pick_files(mime_types: &[&str], multiple: bool) -> Result<Vec<PickedFile>, SentinelError> {
    #[cfg(target_os = "android")]
    {
        android::pick_files_blocking(mime_types, multiple)
            .ok_or_else(|| SentinelError::unavailable("media_picker::pick_files"))
    }
    #[cfg(windows)]
    {
        Ok(windows::pick_files(mime_types, multiple))
    }
    #[cfg(not(any(target_os = "android", windows)))]
    {
        let _ = (mime_types, multiple);
        Err(SentinelError::unavailable("media_picker::pick_files"))
    }
}

/// Best-effort MIME type from a file extension (shared by the desktop
/// backends; Android asks the content resolver instead).
#[cfg(any(windows, test))]
pub(crate) fn mime_from_name(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "bmp" => "image/bmp",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

#[cfg(windows)]
mod windows {
    use super::{mime_from_name, PickedFile};

    /// Extensions offered for a MIME filter like `image/*`.
    fn extensions_for(mime: &str) -> &'static [&'static str] {
        match mime {
            "image/*" => &["jpg", "jpeg", "png", "gif", "webp", "bmp", "heic"],
            "video/*" => &["mp4", "m4v", "webm", "mov", "mkv"],
            "audio/*" => &["mp3", "m4a", "aac", "ogg", "opus", "wav", "flac"],
            _ => &[],
        }
    }

    pub(super) fn pick_files(mime_types: &[&str], multiple: bool) -> Vec<PickedFile> {
        let mut dialog = rfd::FileDialog::new();
        let exts: Vec<&str> = mime_types
            .iter()
            .flat_map(|m| extensions_for(m).iter().copied())
            .collect();
        if !exts.is_empty() {
            dialog = dialog.add_filter("Media", &exts);
        }
        let paths = if multiple {
            dialog.pick_files().unwrap_or_default()
        } else {
            dialog.pick_file().into_iter().collect()
        };
        paths
            .into_iter()
            .filter_map(|p| {
                let size = std::fs::metadata(&p).ok()?.len();
                let name = p.file_name()?.to_string_lossy().into_owned();
                Some(PickedFile {
                    mime: mime_from_name(&name).to_string(),
                    path: p.to_string_lossy().into_owned(),
                    name,
                    size,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::mime_from_name;

    #[test]
    fn mime_from_common_names() {
        assert_eq!(mime_from_name("a.JPG"), "image/jpeg");
        assert_eq!(mime_from_name("clip.mp4"), "video/mp4");
        assert_eq!(mime_from_name("note.m4a"), "audio/mp4");
        assert_eq!(mime_from_name("noext"), "application/octet-stream");
    }
}

#[cfg(target_os = "android")]
mod android {
    /// Only for a picker that never came up: what is picked after this is lost.
    const WAIT_SECS: i64 = 30 * 60;

    use super::PickedFile;
    use crate::platform::android::context::get_activity;
    use crate::platform::android::jni::resolve_helper_class;
    use jni::objects::{JClass, JString, JValue};

    const FILE_PICKER: &str = "com/mobilesentinel/SentinelFilePickerHelper";

    /// Blocking multi-pick with metadata. `None` if the picker could not be
    /// launched; `Some(vec![])` on cancel/timeout.
    pub(super) fn pick_files_blocking(mimes: &[&str], multiple: bool) -> Option<Vec<PickedFile>> {
        let (vm, activity) = get_activity().ok()?;
        let mut env = vm.attach_current_thread_permanently().ok()?;
        let class = resolve_helper_class(&mut env, FILE_PICKER)?;
        let class_ref: &JClass = <&JClass>::from(class.as_obj());
        let csv = if mimes.is_empty() {
            "*/*".to_string()
        } else {
            mimes.join(",")
        };
        let s_mimes = env.new_string(csv).ok()?;
        let res = env.call_static_method(
            class_ref,
            "pickFilesInfoBlocking",
            "(Landroid/app/Activity;Ljava/lang/String;ZJ)Ljava/lang/String;",
            &[
                JValue::Object(activity.as_obj()),
                JValue::Object(&s_mimes.into()),
                JValue::Bool(multiple as u8),
                JValue::Long(WAIT_SECS),
            ],
        );
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
            return None;
        }
        let obj = res.ok()?.l().ok()?;
        if obj.is_null() {
            return Some(Vec::new());
        }
        let jstr: JString = obj.into();
        let json: String = env.get_string(&jstr).ok()?.into();
        Some(serde_json::from_str(&json).unwrap_or_default())
    }

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
                JValue::Long(WAIT_SECS),
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

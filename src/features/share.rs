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

/// A mail to compose in the user's own email app. Nothing is sent by this crate: the user
/// presses send there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email<'a> {
    pub to: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
    /// Absolute paths of files under the app's files or cache directory.
    pub attachments: &'a [&'a str],
}

impl Email<'_> {
    /// The attachments as one string for the platform side, one path per line. `None` when
    /// the mail cannot be made: no address, or a path with a newline in it.
    pub fn attachment_list(&self) -> Option<String> {
        if !self.to.contains('@') || self.to.contains(['\n', '\r', ' ']) {
            return None;
        }
        if self
            .attachments
            .iter()
            .any(|p| p.is_empty() || p.contains(['\n', '\r']))
        {
            return None;
        }
        Some(self.attachments.join("\n"))
    }
}

/// Compose an email in the user's email app with `attachments`, through
/// `ACTION_SEND_MULTIPLE` and the share FileProvider. Returns `Err` when there is no email app
/// (the app should then offer another way, such as [`save_to_downloads`]). The app must declare
/// `<queries>` for the intent `android.intent.action.SEND_MULTIPLE` with `message/rfc822`, or
/// Android 11+ hides the email apps from this check.
pub fn email(
    to: &str,
    subject: &str,
    body: &str,
    attachments: &[&str],
) -> Result<(), SentinelError> {
    let mail = Email {
        to,
        subject,
        body,
        attachments,
    };
    let Some(list) = mail.attachment_list() else {
        return Err(SentinelError::unavailable("share::email"));
    };
    #[cfg(target_os = "android")]
    {
        if android::email(to, subject, body, &list) {
            return Ok(());
        }
        Err(SentinelError::unavailable("share::email"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = list;
        Err(SentinelError::unavailable("share::email"))
    }
}

/// Open a file with the app the system associates with its type (a PDF
/// viewer, a video player, ...). Android: `ACTION_VIEW`; Windows: the
/// default program.
pub fn open(path: &str, mime_type: &str) -> Result<(), SentinelError> {
    #[cfg(target_os = "android")]
    {
        if android::open_file(path, mime_type) {
            return Ok(());
        }
        Err(SentinelError::unavailable("share::open"))
    }
    #[cfg(windows)]
    {
        let _ = mime_type;
        opener::open(path).map_err(|e| SentinelError::RuntimeError {
            code: crate::error::ErrorCode::IoError,
            message: e.to_string(),
        })
    }
    #[cfg(not(any(target_os = "android", windows)))]
    {
        let _ = (path, mime_type);
        Err(SentinelError::unavailable("share::open"))
    }
}

/// Open a web address (http/https) in the user's browser.
pub fn open_url(url: &str) -> Result<(), SentinelError> {
    if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("market://"))
    {
        return Err(SentinelError::unavailable("share::open_url"));
    }
    #[cfg(target_os = "android")]
    {
        if android::open_url(url) {
            return Ok(());
        }
        Err(SentinelError::unavailable("share::open_url"))
    }
    #[cfg(windows)]
    {
        opener::open_browser(url).map_err(|e| SentinelError::RuntimeError {
            code: crate::error::ErrorCode::IoError,
            message: e.to_string(),
        })
    }
    #[cfg(not(any(target_os = "android", windows)))]
    {
        Err(SentinelError::unavailable("share::open_url"))
    }
}

/// Copy a file into the user's Downloads folder as `name`. Returns a
/// human-readable location of the copy (e.g. `Download/report.pdf`).
pub fn save_to_downloads(path: &str, name: &str, mime_type: &str) -> Result<String, SentinelError> {
    #[cfg(target_os = "android")]
    {
        let saved = android::save_to_downloads(path, name, mime_type);
        if saved.is_empty() {
            Err(SentinelError::unavailable("share::save_to_downloads"))
        } else {
            Ok(saved)
        }
    }
    #[cfg(windows)]
    {
        let _ = mime_type;
        windows::save_to_downloads(path, name).map_err(|e| SentinelError::RuntimeError {
            code: crate::error::ErrorCode::IoError,
            message: e.to_string(),
        })
    }
    #[cfg(not(any(target_os = "android", windows)))]
    {
        let _ = (path, name, mime_type);
        Err(SentinelError::unavailable("share::save_to_downloads"))
    }
}

/// A file name that does not exist yet in `dir`: `name`, `name (1)`, ...
#[cfg(any(windows, test))]
fn unique_in(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut candidate = dir.join(name);
    let mut n = 1;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    candidate
}

#[cfg(windows)]
mod windows {
    pub(super) fn save_to_downloads(path: &str, name: &str) -> std::io::Result<String> {
        let dir =
            dirs::download_dir().ok_or_else(|| std::io::Error::other("no Downloads folder"))?;
        // Only the file name part: never let a received name escape the folder.
        let safe = std::path::Path::new(name)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "download".into());
        let dest = super::unique_in(&dir, &safe);
        std::fs::copy(path, &dest)?;
        Ok(dest.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_email_needs_an_address_and_clean_paths() {
        let mail = |to, paths| Email {
            to,
            subject: "s",
            body: "b",
            attachments: paths,
        };
        assert_eq!(
            mail("a@b.com", &["/f/one.txt", "/f/two.json"]).attachment_list(),
            Some("/f/one.txt\n/f/two.json".to_owned())
        );
        assert_eq!(mail("a@b.com", &[]).attachment_list(), Some(String::new()));
        assert_eq!(mail("", &[]).attachment_list(), None);
        assert_eq!(mail("a b@c.com", &[]).attachment_list(), None);
        assert_eq!(mail("a@b.com", &["/x\ny"]).attachment_list(), None);
        assert!(email("nobody", "s", "b", &[]).is_err());
    }

    #[test]
    fn open_url_takes_web_and_store_addresses_only() {
        assert!(open_url("file:///etc/passwd").is_err());
        assert!(open_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn unique_names_do_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            super::unique_in(dir.path(), "a.pdf"),
            dir.path().join("a.pdf")
        );
        std::fs::write(dir.path().join("a.pdf"), b"x").unwrap();
        assert_eq!(
            super::unique_in(dir.path(), "a.pdf"),
            dir.path().join("a (1).pdf")
        );
        std::fs::write(dir.path().join("a (1).pdf"), b"x").unwrap();
        assert_eq!(
            super::unique_in(dir.path(), "a.pdf"),
            dir.path().join("a (2).pdf")
        );
        assert_eq!(
            super::unique_in(dir.path(), "noext"),
            dir.path().join("noext")
        );
    }
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::{jni_str, with_jni_class};
    use jni::objects::{JObject, JString, JValue};
    use jni::JNIEnv;

    pub(super) fn open_file(path: &str, mime_type: &str) -> bool {
        with_jni_class(SHARE, false, |env, class| {
            let s_path = jni_str(env, path)?;
            let s_mime = jni_str(env, mime_type)?;
            env.call_static_method(
                class,
                "openFile",
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

    pub(super) fn email(to: &str, subject: &str, body: &str, paths: &str) -> bool {
        with_jni_class(SHARE, false, |env, class| {
            let s_to = jni_str(env, to)?;
            let s_subject = jni_str(env, subject)?;
            let s_body = jni_str(env, body)?;
            let s_paths = jni_str(env, paths)?;
            env.call_static_method(
                class,
                "shareEmail",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Z",
                &[
                    JValue::Object(&s_to.into()),
                    JValue::Object(&s_subject.into()),
                    JValue::Object(&s_body.into()),
                    JValue::Object(&s_paths.into()),
                ],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn open_url(url: &str) -> bool {
        with_jni_class(SHARE, false, |env, class| {
            let s_url = jni_str(env, url)?;
            env.call_static_method(
                class,
                "openUrl",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&s_url.into())],
            )
            .ok()
            .and_then(|v| v.z().ok())
        })
    }

    pub(super) fn save_to_downloads(path: &str, name: &str, mime_type: &str) -> String {
        with_jni_class(SHARE, String::new(), |env, class| {
            let s_path = jni_str(env, path)?;
            let s_name = jni_str(env, name)?;
            let s_mime = jni_str(env, mime_type)?;
            let obj = env
                .call_static_method(
                    class,
                    "saveToDownloads",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                    &[
                        JValue::Object(&s_path.into()),
                        JValue::Object(&s_name.into()),
                        JValue::Object(&s_mime.into()),
                    ],
                )
                .ok()?
                .l()
                .ok()?;
            let jstr: JString = obj.into();
            env.get_string(&jstr).ok().map(Into::into)
        })
    }

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

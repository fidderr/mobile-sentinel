//! Camera capability — take a photo with the system camera app.
//!
//! This module is the **only** public entry point for photo capture. It is
//! gated behind the `camera` Cargo feature, so a consumer that calls
//! [`capture_photo`] without enabling `camera` gets a compile error at the
//! call site.
//!
//! ```toml
//! [dependencies]
//! mobile-sentinel = { version = "...", features = ["camera"] }
//! ```
//!
//! Capture delegates to the device's installed camera app via
//! `Intent.ACTION_IMAGE_CAPTURE` — mobile-sentinel does **not** ship its own
//! camera UI here. This keeps the capability tiny (no CameraX/ZXing
//! dependency) and uses the camera experience the user already knows. The
//! resulting full-resolution JPEG is written to the app's internal storage and
//! its path returned.
//!
//! This is intentionally separate from the [`crate::scanner`] capability
//! (barcode / QR decoding): wanting to take a photo should not pull in the
//! ZXing scanner, and wanting to scan a code should not imply a generic camera.

use crate::error::SentinelError;

/// Take a photo with the system camera app, returning the absolute path to the
/// saved JPEG in the app's internal storage.
///
/// Blocks until the user takes a photo or cancels. Returns
/// [`SentinelError::PlatformUnsupported`] when no camera app is available
/// (host builds, or a cancelled capture on device).
///
/// Requires the `camera` feature. The `SentinelCameraActivity` proxy +
/// `FileProvider` are wired automatically by `build_sentinel` because the
/// feature is enabled — see [`crate::build::registry`].
pub fn capture_photo() -> Result<String, SentinelError> {
    #[cfg(target_os = "android")]
    {
        let path = android::capture_photo();
        if path.is_empty() {
            Err(SentinelError::unavailable("camera::capture_photo"))
        } else {
            Ok(path)
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        Err(SentinelError::unavailable("camera::capture_photo"))
    }
}

/// `true` if photo capture is backed on this platform/build.
pub fn is_available() -> bool {
    cfg!(target_os = "android")
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;
    use jni::objects::{JString, JValue};

    const HELPER: &str = "com/mobilesentinel/SentinelCameraHelper";

    /// Blocking photo capture from the app context. Returns the absolute path
    /// to the saved JPEG, or empty on cancel/timeout.
    pub(super) fn capture_photo() -> String {
        with_jni_class(HELPER, String::new(), |env, class| {
            let res = env
                .call_static_method(
                    class,
                    "captureFromAppContext",
                    "(J)Ljava/lang/String;",
                    &[JValue::Long(120)],
                )
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        })
    }
}

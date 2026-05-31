//! Scanner capability — barcode / QR scanning.
//!
//! This module is the **only** public entry point for scanning. It is
//! gated behind the `scanner` Cargo feature, so a consumer that calls
//! [`scan`] without enabling `scanner` gets a compile error at the
//! call site — the SDK cannot be used beyond what the consumer declared.
//!
//! ```toml
//! [dependencies]
//! mobile-sentinel = { version = "...", features = ["scanner"] }
//! ```
//!
//! Scanning is intentionally NOT reachable through any raw sink handle:
//! it lives only behind this feature-gated module, so the gate cannot be
//! bypassed. It is distinct from the [`crate::camera`] capability (system
//! photo capture) — a consumer that only wants to take a photo does not pull
//! in the ZXing scanner, and vice versa.

use crate::error::SentinelError;

/// Scan a barcode / QR code, returning the decoded value.
///
/// Blocks until the user completes or cancels the scan. Returns
/// [`SentinelError::PlatformUnsupported`] when no scanner is available
/// (host builds, or a cancelled/timed-out scan on device).
///
/// Requires the `scanner` feature. The corresponding Android manifest
/// pieces (CAMERA permission, `SentinelScannerActivity`) and the ZXing
/// dependency are wired automatically by `build_sentinel` because the
/// feature is enabled — see [`crate::build::registry`].
pub fn scan() -> Result<String, SentinelError> {
    #[cfg(target_os = "android")]
    {
        let value = android::scan();
        if value.is_empty() {
            Err(SentinelError::unavailable("scanner::scan"))
        } else {
            Ok(value)
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        Err(SentinelError::unavailable("scanner::scan"))
    }
}

/// `true` if barcode scanning is backed on this platform/build.
pub fn is_available() -> bool {
    cfg!(target_os = "android")
}

#[cfg(target_os = "android")]
mod android {
    use crate::platform::android::jni::with_jni_class;
    use jni::objects::{JString, JValue};

    const SCANNER: &str = "com/mobilesentinel/SentinelScannerHelper";

    /// Blocking barcode/QR scan from the app context. Returns the decoded
    /// string, or empty on cancel/timeout.
    pub(super) fn scan() -> String {
        with_jni_class(SCANNER, String::new(), |env, class| {
            let res = env
                .call_static_method(
                    class,
                    "scanFromAppContext",
                    "(J)Ljava/lang/String;",
                    &[JValue::Long(60)],
                )
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        })
    }
}

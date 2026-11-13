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
/// Blocks until the user completes or cancels the scan, which can take a while: the code can be
/// in another room. Returns [`SentinelError::PlatformUnsupported`] when no scanner is available
/// (host builds, or a cancelled scan on device).
///
/// Requires the `scanner` feature. The corresponding Android manifest
/// pieces (CAMERA permission, `SentinelScannerActivity`) and the ZXing
/// dependency are wired automatically by `build_sentinel` because the
/// feature is enabled — see [`crate::build::registry`].
pub fn scan() -> Result<String, SentinelError> {
    scan_with(0)
}

/// As [`scan`], but the camera screen closes itself after `limit`, as Back would, and nothing is
/// scanned. The scanner keeps the time itself, so it holds while the caller's own screen is
/// covered by the camera and not drawing.
pub fn scan_for(limit: std::time::Duration) -> Result<String, SentinelError> {
    scan_with(limit.as_millis().clamp(1, i64::MAX as u128) as i64)
}

fn scan_with(close_after_ms: i64) -> Result<String, SentinelError> {
    #[cfg(target_os = "android")]
    {
        let value = android::scan(close_after_ms);
        if value.is_empty() {
            Err(SentinelError::unavailable("scanner::scan"))
        } else {
            Ok(value)
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = close_after_ms;
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
    /// Only for a scanner that never came up: what is scanned after this is lost.
    const WAIT_SECS: i64 = 30 * 60;

    /// Blocking barcode/QR scan from the app context. Returns the decoded
    /// string, or empty on cancel/timeout. `close_after_ms` above 0 closes the camera then.
    pub(super) fn scan(close_after_ms: i64) -> String {
        with_jni_class(SCANNER, String::new(), |env, class| {
            let res = env
                .call_static_method(
                    class,
                    "scanFromAppContext",
                    "(JJ)Ljava/lang/String;",
                    &[JValue::Long(WAIT_SECS), JValue::Long(close_after_ms)],
                )
                .ok()?;
            let obj = res.l().ok()?;
            let jstr: JString = obj.into();
            let s: String = env.get_string(&jstr).ok()?.into();
            Some(s)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_scanner_both_ways_say_unavailable() {
        if cfg!(target_os = "android") {
            return;
        }
        assert!(scan().is_err());
        assert!(scan_for(std::time::Duration::from_secs(60)).is_err());
        assert!(scan_for(std::time::Duration::ZERO).is_err());
    }
}

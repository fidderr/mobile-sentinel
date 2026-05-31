//! Structured error types for mobile-sentinel operations.

use std::fmt;

/// Platform identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Android,
    Unsupported,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Android => write!(f, "Android"),
            Self::Unsupported => write!(f, "Unsupported"),
        }
    }
}

/// Error codes for runtime errors.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// A required resource is currently in use by another operation.
    ResourceBusy,
    /// An argument provided to the operation was invalid.
    InvalidArgument,
    /// The operation timed out before completing.
    Timeout,
    /// An internal error occurred within the subsystem.
    Internal,
    /// An I/O error occurred (file not found, network failure, etc.).
    IoError,
    /// The operation is not supported on this platform.
    Unsupported,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResourceBusy => write!(f, "ResourceBusy"),
            Self::InvalidArgument => write!(f, "InvalidArgument"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Internal => write!(f, "Internal"),
            Self::IoError => write!(f, "IoError"),
            Self::Unsupported => write!(f, "Unsupported"),
        }
    }
}

/// Top-level error type for all mobile-sentinel operations.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SentinelError {
    /// The operation is not supported on this platform / has no backing
    /// implementation in the installed [`crate::FiringSink`] or capability
    /// module.
    #[error("platform unsupported: {feature} is not available on {platform}")]
    PlatformUnsupported {
        platform: Platform,
        feature: String,
        fallback_suggestion: Option<String>,
    },

    /// A runtime operation failed.
    #[error("runtime error [{code}]: {message}")]
    RuntimeError { code: ErrorCode, message: String },
}

impl SentinelError {
    /// Construct the standard "this capability isn't available here" error
    /// used by every feature-gated capability module on its host (non-Android)
    /// fallback path. Centralised so capability modules don't each re-roll
    /// the same `PlatformUnsupported { .. }` literal.
    ///
    /// `feature` should be the gated entry point, e.g. `"haptics::vibrate"`.
    pub fn unavailable(feature: &str) -> Self {
        SentinelError::PlatformUnsupported {
            platform: if cfg!(target_os = "android") {
                Platform::Android
            } else {
                Platform::Unsupported
            },
            feature: feature.to_string(),
            fallback_suggestion: None,
        }
    }
}

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn runtime_error_renders_code_and_message() {
        let e = SentinelError::RuntimeError {
            code: ErrorCode::Internal,
            message: "boom".into(),
        };
        assert_eq!(e.to_string(), "runtime error [Internal]: boom");
    }

    #[test]
    fn platform_unsupported_renders_feature() {
        let e = SentinelError::PlatformUnsupported {
            platform: Platform::Unsupported,
            feature: "speech::speak".into(),
            fallback_suggestion: None,
        };
        assert!(e.to_string().contains("speech::speak"));
    }
}

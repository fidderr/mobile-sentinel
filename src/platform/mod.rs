//! Platform-specific implementations.
//! This module contains cfg-gated platform backends. On Android, the `android`
//! module provides JNI-based implementations.

#[cfg(target_os = "android")]
pub mod android;

// Re-export the callbacks module with cross-platform stubs so apps can
// unconditionally call on_boot_completed(), on_job_heads_up(), etc.
// On non-Android platforms these are no-ops.
#[cfg(not(target_os = "android"))]
pub mod callbacks_stub;

/// Cross-platform access to the callback registration API.
/// On Android: registers real JNI callbacks that fire when the OS delivers events.
/// On other platforms: no-ops (for development/testing).
pub mod callbacks {
    #[cfg(target_os = "android")]
    pub use super::android::callbacks::*;

    #[cfg(not(target_os = "android"))]
    pub use super::callbacks_stub::*;
}

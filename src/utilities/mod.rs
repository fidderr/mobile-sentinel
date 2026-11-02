//! Utility types for common mobile app patterns.

pub mod app_dirs;
pub mod asset_extractor;
pub mod boot;
// Only the on-disk stores use it.
#[cfg(any(feature = "jobs", feature = "state-store"))]
pub(crate) mod atomic_write;

pub use app_dirs::{app_files_dir, device_language};
pub use asset_extractor::AssetExtractor;

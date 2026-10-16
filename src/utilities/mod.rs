//! Utility types for common mobile app patterns.

pub mod app_dirs;
pub mod asset_extractor;
// Only the on-disk stores use it.
#[cfg(any(feature = "jobs", feature = "state-store"))]
pub(crate) mod atomic_write;

pub use app_dirs::app_files_dir;
pub use asset_extractor::AssetExtractor;

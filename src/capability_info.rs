//! The capability list kept inside the app's native library.
//!
//! `build.rs` records the Cargo features mobile-sentinel was compiled with, and this static keeps
//! that list as a string in the library itself, for every target. `build_sentinel` reads it back
//! from the `.so` it is about to package (`build::embedded`, host tooling behind the `build`
//! feature).

/// What every embedded list starts with. The list follows, comma separated, then `]`.
pub const MARKER: &str = "mobile-sentinel-capabilities:v1[";

/// Kept in the library: exported, so neither the compiler nor the linker drops it.
#[no_mangle]
#[used]
pub static MOBILE_SENTINEL_CAPABILITIES_INFO: &str = concat!(
    "mobile-sentinel-capabilities:v1[",
    env!("MOBILE_SENTINEL_CAPABILITY_LIST"),
    "]"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_build_keeps_the_list_in_the_library() {
        assert!(MOBILE_SENTINEL_CAPABILITIES_INFO.starts_with(MARKER));
        assert!(MOBILE_SENTINEL_CAPABILITIES_INFO.ends_with(']'));
    }
}

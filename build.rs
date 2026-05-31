//! Build script: record which capability features were enabled so that
//! `build_sentinel` can derive the Android manifest, permissions, and the
//! per-capability Gradle module wiring from the exact same set the Rust
//! compiler used.
//!
//! Cargo sets `CARGO_FEATURE_<NAME>=1` for every enabled feature. We read
//! those, write the enabled capability ids (lower-snake-case) to a file in
//! OUT_DIR, and also re-export the path via a cargo:rustc-env so the
//! library can locate it at build time if needed.
//!
//! There is no hand-paired (feature, id) table here: the capability ids live
//! in `src/build/capability_ids.rs`, included verbatim below (the SAME file
//! the runtime `registry` module includes). The feature↔id relationship is
//! mechanical — `CARGO_FEATURE_<ID_UPPER>` for each id — so build-script and
//! runtime can never disagree about the set. The capability *metadata*
//! (permissions, components, Kotlin modules) still lives only in
//! `src/build/registry.rs::CAPABILITIES`.

use std::env;
use std::fs;
use std::path::Path;

// The single shared capability-id list — see src/build/capability_ids.rs.
// Brings `CAPABILITY_IDS` and `FIRING_BUNDLE_IDS` into scope.
include!("src/build/capability_ids.rs");

/// `CARGO_FEATURE_*` env suffix for a capability id: upper-case it. Cargo
/// upper-cases feature names and replaces `-` with `_`; our ids are already
/// snake_case, so upper-casing the id yields the exact env suffix Cargo sets
/// (e.g. id `full_screen_intent` → `CARGO_FEATURE_FULL_SCREEN_INTENT`).
fn feature_env_suffix(capability_id: &str) -> String {
    capability_id.to_uppercase()
}

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set by cargo");
    let dest = Path::new(&out_dir).join("enabled_capabilities.txt");

    let mut enabled: Vec<&str> = Vec::new();
    for cap in CAPABILITY_IDS {
        // Cargo exposes enabled features as CARGO_FEATURE_<UPPER_SNAKE>.
        if env::var(format!("CARGO_FEATURE_{}", feature_env_suffix(cap))).is_ok() {
            enabled.push(cap);
        }
    }

    let contents = enabled.join("\n");
    fs::write(&dest, &contents).expect("failed to write enabled_capabilities.txt");

    // Emit a `firing_enabled` cfg when any firing sub-feature is on, so the
    // FiringSink trait + AndroidFiringSink compile for any firing surface
    // without each call site repeating the long `any(feature=...)` list.
    println!("cargo:rustc-check-cfg=cfg(firing_enabled)");
    if FIRING_BUNDLE_IDS
        .iter()
        .any(|id| env::var(format!("CARGO_FEATURE_{}", feature_env_suffix(id))).is_ok())
    {
        println!("cargo:rustc-cfg=firing_enabled");
    }

    // Make the path discoverable to the crate / downstream tooling.
    println!(
        "cargo:rustc-env=MOBILE_SENTINEL_ENABLED_CAPABILITIES={}",
        dest.display()
    );
    // Also surface the resolved set as a build-time env for convenience.
    println!("cargo:rustc-env=MOBILE_SENTINEL_CAPABILITIES={contents}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/build/capability_ids.rs");
}

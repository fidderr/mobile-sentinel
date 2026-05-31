// THE capability id list — the single primitive shared by `build.rs` (the
// build script, which cannot link the crate) and the runtime `registry`
// module. Included VERBATIM by both via `include!`, so there is exactly one
// place to edit when adding a capability.
//
// Each entry is a capability id in `lower_snake_case`. The relationship to a
// consumer's Cargo feature is mechanical, not a lookup table:
//
//   * Cargo feature name      = id with `_`→`-`   (e.g. `full_screen_intent`
//                                                  → feature `full-screen-intent`)
//   * `CARGO_FEATURE_*` env    = id upper-cased     (e.g. → `FULL_SCREEN_INTENT`)
//
// Because both conversions are pure functions of the id, neither `build.rs`
// nor `registry` needs a hand-paired (feature, id) table. `build.rs` reads
// `CARGO_FEATURE_{ID_UPPER}` for each id; `registry::cargo_feature_to_capability`
// snake-cases the feature name and looks it up in `CAPABILITIES`.
//
// A drift test in `registry.rs` (`capability_ids_match_capabilities_table`)
// asserts this list and the `CAPABILITIES` table never disagree, so adding a
// row to one without the other fails the build.

/// Every capability id mobile-sentinel knows. Order is irrelevant (both
/// consumers treat it as a set); grouped here for readability.
pub const CAPABILITY_IDS: &[&str] = &[
    // Leaf capabilities (each has a feature-gated Rust door + Kotlin module).
    "scanner",
    "camera",
    "haptics",
    "audio",
    "overlay",
    "permissions",
    "sensors",
    "torch",
    "display",
    "battery",
    "screen_pin",
    "foregrounding",
    "media_picker",
    "clipboard",
    "share",
    "secure_storage",
    "sms",
    "phone",
    "network",
    "location",
    "maps",
    "biometric",
    "device_admin",
    "contacts",
    "calendar",
    "dismiss_guard",
    "notifications",
    "file_system",
    "accessibility",
    // Firing sub-features — each its own per-feature Kotlin/Gradle module
    // under android/caps/<id> (no shared "alarm runtime" bundle).
    "foreground_service",
    "firing_audio",
    "firing_vibration",
    "wake_lock",
    "exact_alarm",
    "kiosk",
    "full_screen_intent",
    // Building blocks with a Kotlin surface.
    "jobs",
];

/// The firing sub-feature ids the `firing` convenience bundle expands to (it
/// composes the complete firing surface). Mirrors the Cargo `firing = [...]`
/// list so `--app` capability derivation matches a feature-compiled `.so`,
/// and so `build.rs` can emit the `firing_enabled` cfg without a second list.
pub const FIRING_BUNDLE_IDS: &[&str] = &[
    "foreground_service",
    "firing_audio",
    "firing_vibration",
    "wake_lock",
    "exact_alarm",
    "kiosk",
    "full_screen_intent",
];

//! Build-time tooling (NOT runtime code).
//!
//! This module groups everything `build_sentinel` uses to assemble a
//! consumer's Android project from its enabled capabilities:
//!
//! - [`registry`] — the capability → manifest (permissions / components) +
//!   Kotlin / Gradle trim table. The source of truth for what each
//!   capability contributes to the APK.
//! - [`embedded`] — the capability list kept inside the app's `.so`, read back by `build_sentinel`.
//! - [`helper`] — wires mobile-sentinel's Gradle module into a host Android
//!   project and injects the capability-derived manifest fragments.
//! - [`host`] — generates the whole Gradle project for an app that keeps none.
//! - [`shims`] — one-line Java classes for components whose code is Rust.
//!
//! None of this is reachable at runtime by a consumer app; it runs on the
//! build host via the `build_sentinel` binary.

pub mod declare;
pub mod embedded;
pub mod helper;
pub mod host;
pub mod registry;
pub mod shims;

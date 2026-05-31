//! Build helper for integrating mobile-sentinel into Android apps.
//!
//! Uses the AAR/Gradle module approach:
//!
//! 1. Includes mobile-sentinel's `android/` as a Gradle module in the project.
//! 2. Adds it as a dependency to the app module.
//! 3. Gradle automatically merges the manifest, compiles Kotlin, and bundles resources.
//!
//! No manual XML parsing, no file copying. Gradle handles everything.
//!
//! # Usage
//!
//! ```ignore
//! // After building the Android project (e.g. `dx build --platform android`):
//! mobile_sentinel::build::helper::prepare_android_project(
//! "target/dx/myapp/debug/android/app",
//! "com.example.app.MainActivity",
//! );
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use crate::build::registry::{self as capabilities, ManifestInjection};

/// Marker comment used to make capability injection idempotent across
/// repeated `build_sentinel` runs.
const INJECTION_MARKER: &str = "<!-- mobile-sentinel:capabilities -->";

/// Marker used to make the Gradle module wiring (settings.gradle includes +
/// app dependency lines) idempotent across repeated `build_sentinel` runs.
const GRADLE_MARKER: &str = "mobile-sentinel:modules";

/// Find the mobile-sentinel crate's android directory (absolute path).
fn find_sentinel_android_dir() -> Option<PathBuf> {
    let candidates = [
        "../../crates/mobile-sentinel/android",
        "../crates/mobile-sentinel/android",
        "crates/mobile-sentinel/android",
    ];

    for candidate in &candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            // Use canonicalize but strip the \\?\ prefix on Windows
            if let Ok(abs) = fs::canonicalize(&path) {
                let abs_str = abs.to_string_lossy().to_string();
                let cleaned = abs_str
                    .strip_prefix(r"\\?\")
                    .unwrap_or(&abs_str)
                    .to_string();
                return Some(PathBuf::from(cleaned));
            }
            return Some(path);
        }
    }

    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let path = PathBuf::from(manifest_dir).join("../../crates/mobile-sentinel/android");
        if path.exists() {
            if let Ok(abs) = fs::canonicalize(&path) {
                let abs_str = abs.to_string_lossy().to_string();
                let cleaned = abs_str
                    .strip_prefix(r"\\?\")
                    .unwrap_or(&abs_str)
                    .to_string();
                return Some(PathBuf::from(cleaned));
            }
            return Some(path);
        }
    }

    None
}

/// Prepare a host Android project so it can consume `mobile-sentinel`.
///
/// - Adds `mobile-sentinel/android/` as a Gradle library module.
/// - Declares the `implementation project(':mobile-sentinel')` dependency.
/// - Applies runtime-only activity attributes (`showWhenLocked`,
///   `turnScreenOn`, `launchMode="singleInstance"`) to the activity named
///   by `activity_fqcn`. The caller chooses the FQCN; the crate is agnostic.
///
/// This entry point enables **no optional capabilities** — only the
/// baseline manifest is merged. Use [`prepare_android_project_with_capabilities`]
/// to opt into camera, overlay, accessibility, device-admin, etc.
///
/// The consuming app passes its own entry-point activity FQCN (for example,
/// `com.example.app.MainActivity`).
///
/// Panics if the `android/` directory of the crate cannot be located —
/// that is a build-system misconfiguration, not a runtime concern.
pub fn prepare_android_project(android_project_path: &str, activity_fqcn: &str) {
    prepare_android_project_with_capabilities(android_project_path, activity_fqcn, &[] as &[&str]);
}

/// Like [`prepare_android_project`], but additionally injects the
/// permissions, hardware features, and `<application>` components for each
/// declared capability id (see [`crate::capabilities`]) into the
/// consumer's manifest.
///
/// Capabilities the consumer does not declare are never injected, so they
/// never appear in the shipped APK's merged manifest. Unknown capability
/// ids are returned in [`ManifestInjection::unknown`] so the caller can
/// surface them; policy-sensitive ids are returned in
/// [`ManifestInjection::policy_warnings`].
///
/// Returns the [`ManifestInjection`] that was applied so the caller
/// (`build_sentinel`) can print a summary.
pub fn prepare_android_project_with_capabilities<S: AsRef<str>>(
    android_project_path: &str,
    activity_fqcn: &str,
    capabilities_declared: &[S],
) -> ManifestInjection {
    let project_path = Path::new(android_project_path);

    let sentinel_android = find_sentinel_android_dir().unwrap_or_else(|| {
        panic!(
            "[mobile-sentinel] could not locate the crate's android/ directory \
             (looked in `../../crates/mobile-sentinel/android`, \
             `../crates/mobile-sentinel/android`, `crates/mobile-sentinel/android`, \
             and CARGO_MANIFEST_DIR/../../crates/mobile-sentinel/android)"
        )
    });

    // Forward slashes for Gradle on Windows.
    let sentinel_path_str = sentinel_android.to_string_lossy().replace('\\', "/");

    // Which capability modules to wire in (core is always included).
    let modules = capabilities::enabled_modules(capabilities_declared);

    add_modules_to_settings(project_path, &sentinel_path_str, &modules);
    add_module_dependencies(project_path, &modules);
    add_activity_flags(project_path, activity_fqcn);

    let injection = capabilities::assemble(capabilities_declared);
    inject_capabilities(project_path, &injection);

    eprintln!("[mobile-sentinel] Android project prepared (multi-module AAR approach)");
    eprintln!(
        "[mobile-sentinel] Gradle will auto-merge manifest, compile Kotlin, bundle resources"
    );

    injection
}

/// Include `:sentinel-core` and each enabled capability module in the
/// consumer's `settings.gradle`, pointing each at its directory under the
/// crate's `android/`. Idempotent: rewrites the marker-delimited block on
/// every run, so toggling capabilities never leaves stale includes.
fn add_modules_to_settings(project_path: &Path, sentinel_path: &str, modules: &[&str]) {
    let settings_path = project_path.join("settings.gradle");
    let content = fs::read_to_string(&settings_path).unwrap_or_default();

    let stripped = strip_marker_block(&content, GRADLE_MARKER);

    let mut block = String::new();
    block.push_str(&format!("// {}\n", GRADLE_MARKER));
    block.push_str("include ':sentinel-core'\n");
    block.push_str(&format!(
        "project(':sentinel-core').projectDir = new File('{}/core')\n",
        sentinel_path
    ));
    for m in modules {
        block.push_str(&format!("include ':sentinel-{m}'\n"));
        block.push_str(&format!(
            "project(':sentinel-{m}').projectDir = new File('{sentinel_path}/caps/{m}')\n"
        ));
    }
    block.push_str(&format!("// {}\n", GRADLE_MARKER));

    let new_content = format!("{}\n{}", stripped.trim_end(), block);
    if let Err(e) = fs::write(&settings_path, new_content) {
        eprintln!("[mobile-sentinel] Failed to update settings.gradle: {}", e);
    } else {
        eprintln!(
            "[mobile-sentinel] settings.gradle: included :sentinel-core + {} capability module(s)",
            modules.len()
        );
    }
}

/// Add `implementation(project(...))` for `:sentinel-core` + each enabled
/// capability module to the app module's `build.gradle.kts`. Idempotent via
/// the marker block.
fn add_module_dependencies(project_path: &Path, modules: &[&str]) {
    let build_path = project_path.join("app/build.gradle.kts");
    let content = match fs::read_to_string(&build_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let stripped = strip_marker_block(&content, GRADLE_MARKER);

    let mut deps = String::new();
    deps.push_str(&format!("    // {}\n", GRADLE_MARKER));
    deps.push_str("    implementation(project(\":sentinel-core\"))\n");
    for m in modules {
        deps.push_str(&format!("    implementation(project(\":sentinel-{m}\"))\n"));
    }
    deps.push_str(&format!("    // {}\n", GRADLE_MARKER));

    let new_content = if let Some(idx) = stripped.find("dependencies {") {
        let insert_at = idx + "dependencies {".len();
        let mut s = stripped.clone();
        s.insert_str(insert_at, &format!("\n{}", deps));
        s
    } else {
        format!("{}\n\ndependencies {{\n{}}}\n", stripped.trim_end(), deps)
    };

    if let Err(e) = fs::write(&build_path, new_content) {
        eprintln!("[mobile-sentinel] Failed to update build.gradle.kts: {}", e);
    } else {
        eprintln!(
            "[mobile-sentinel] build.gradle.kts: depends on :sentinel-core + {} module(s)",
            modules.len()
        );
    }
}

/// Remove a previously written marker-delimited block (a `// MARKER ... //
/// MARKER` span, line-based). Leaves all other content intact.
fn strip_marker_block(content: &str, marker: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut skipping = false;
    for line in content.lines() {
        if line.contains(marker) {
            skipping = !skipping;
            continue;
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Add showWhenLocked, turnScreenOn, and launchMode="singleInstance" to the
/// caller-nominated activity.
/// `activity_fqcn` is the fully-qualified class name of the activity the
/// consumer wants the lock-screen attributes applied to. Rust mobile
/// frameworks (Dioxus, Tauri, etc.) each have their own entry activity FQCN.
/// `singleInstance` is critical: without it, a concurrent `startActivity`
/// (from the Level-2 foregrounding helper) and a notification tap can
/// both spawn an Activity, producing two colliding native event loops
/// and a crash when the first one's EGL context is torn down.
/// `singleInstance` forces Android to reuse the existing instance
/// and deliver subsequent launches via `onNewIntent`.
fn add_activity_flags(project_path: &Path, activity_fqcn: &str) {
    let manifest_path = project_path.join("app/src/main/AndroidManifest.xml");
    let content = match fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let activity_attrs_present =
        content.contains("showWhenLocked") && content.contains("launchMode");
    let app_back_callback_present = content.contains("enableOnBackInvokedCallback");
    if activity_attrs_present && app_back_callback_present {
        return; // Already fully configured
    }

    let mut new_content = if activity_attrs_present {
        content.clone()
    } else {
        let needle = format!("android:name=\"{}\"", activity_fqcn);
        let replacement = format!(
            "{}\n            android:launchMode=\"singleInstance\"\n            android:showWhenLocked=\"true\"\n            android:turnScreenOn=\"true\"",
            needle,
        );
        content.replace(&needle, &replacement)
    };

    // Opt the application into the Predictive-Back system so our
    // OnBackInvokedCallback in SentinelKioskController actually consumes
    // back gestures. Without this, Activity falls back to the legacy
    // `onBackPressed` path which our overlay-priority callback can't
    // intercept and the user can dismiss the alarm with a single back.
    if !app_back_callback_present {
        new_content = new_content.replace(
            "<application",
            "<application android:enableOnBackInvokedCallback=\"true\"",
        );
    }

    if let Err(e) = fs::write(&manifest_path, new_content) {
        eprintln!("[mobile-sentinel] Failed to update manifest: {}", e);
    } else {
        eprintln!(
            "[mobile-sentinel] Added launchMode=singleInstance, showWhenLocked, turnScreenOn to {}",
            activity_fqcn
        );
    }
}

/// Inject capability-derived permissions, features, and `<application>`
/// components into the consumer's `AndroidManifest.xml`.
///
/// Idempotent: a previous injection block (delimited by [`INJECTION_MARKER`])
/// is removed before the new one is written, so repeated `build_sentinel`
/// runs do not accumulate duplicate entries. When `injection` is empty
/// this still strips any stale block, leaving a clean baseline manifest.
fn inject_capabilities(project_path: &Path, injection: &ManifestInjection) {
    let manifest_path = project_path.join("app/src/main/AndroidManifest.xml");
    let content = match fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("[mobile-sentinel] capability injection skipped — manifest not found");
            return;
        }
    };

    let stripped = strip_injection_block(&content);

    let manifest_block = render_manifest_block(injection);
    if manifest_block.is_empty() {
        // Nothing to inject — write back the stripped baseline if it changed.
        if stripped != content {
            let _ = fs::write(&manifest_path, stripped);
        }
        return;
    }

    let new_content = splice_injection(&stripped, injection);
    match fs::write(&manifest_path, new_content) {
        Ok(()) => {
            let perms = injection.manifest_lines.len();
            let comps = injection.application_components.len();
            eprintln!(
                "[mobile-sentinel] Injected {} manifest line(s) and {} component(s) from capabilities",
                perms, comps
            );
        }
        Err(e) => eprintln!("[mobile-sentinel] Failed to inject capabilities: {}", e),
    }
}

/// Remove a previously injected block (both the manifest-scope and the
/// application-scope segments) delimited by [`INJECTION_MARKER`].
fn strip_injection_block(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut skipping = false;
    for line in content.lines() {
        if line.contains(INJECTION_MARKER) {
            // Toggle: opening marker starts skipping, closing marker ends it.
            skipping = !skipping;
            continue;
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Render the `<manifest>`-scope lines (permissions + features) as a single
/// marker-delimited block, or empty string if there are none.
fn render_manifest_block(injection: &ManifestInjection) -> String {
    if injection.manifest_lines.is_empty() {
        return String::new();
    }
    let mut block = format!("    {}\n", INJECTION_MARKER);
    for line in &injection.manifest_lines {
        block.push_str(line);
        block.push('\n');
    }
    block.push_str(&format!("    {}\n", INJECTION_MARKER));
    block
}

/// Splice both injection segments into the stripped manifest:
/// - permission/feature lines just before the `<application` tag,
/// - component XML just before the closing `</application>` tag.
fn splice_injection(stripped: &str, injection: &ManifestInjection) -> String {
    let mut result = stripped.to_string();

    // 1. Manifest-scope (permissions + features) before <application.
    let manifest_block = render_manifest_block(injection);
    if !manifest_block.is_empty() {
        if let Some(idx) = result.find("<application") {
            // Back up to the start of the line containing <application.
            let line_start = result[..idx].rfind('\n').map(|n| n + 1).unwrap_or(0);
            result.insert_str(line_start, &manifest_block);
        }
    }

    // 2. Application-scope components before </application>.
    if !injection.application_components.is_empty() {
        let mut comp_block = format!("        {}\n", INJECTION_MARKER);
        for comp in &injection.application_components {
            comp_block.push_str(comp);
            comp_block.push('\n');
        }
        comp_block.push_str(&format!("        {}\n", INJECTION_MARKER));

        if let Some(idx) = result.find("</application>") {
            let line_start = result[..idx].rfind('\n').map(|n| n + 1).unwrap_or(0);
            result.insert_str(line_start, &comp_block);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::registry::assemble;

    const BASELINE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <uses-permission android:name="android.permission.WAKE_LOCK" />
    <application>
        <service android:name=".SentinelForegroundService" />
    </application>
</manifest>
"#;

    #[test]
    fn empty_injection_leaves_no_marker() {
        let injection = assemble::<&str>(&[]);
        let out = splice_injection(BASELINE, &injection);
        assert!(!out.contains(INJECTION_MARKER));
        // Baseline content preserved.
        assert!(out.contains("SentinelForegroundService"));
    }

    #[test]
    fn scanner_injection_adds_permission_and_activity() {
        let injection = assemble(&["scanner"]);
        let out = splice_injection(BASELINE, &injection);
        assert!(out.contains("android.permission.CAMERA"));
        assert!(out.contains("com.mobilesentinel.SentinelScannerActivity"));
        // Permission must be inside <manifest> but before <application>.
        let cam = out.find("android.permission.CAMERA").unwrap();
        let app = out.find("<application").unwrap();
        assert!(cam < app, "permission must precede <application>");
        // Activity must be before </application>.
        let act = out.find("SentinelScannerActivity").unwrap();
        let close = out.find("</application>").unwrap();
        assert!(act < close, "component must precede </application>");
    }

    #[test]
    fn injection_is_idempotent() {
        let injection = assemble(&["scanner", "overlay"]);
        let once = splice_injection(&strip_injection_block(BASELINE), &injection);
        let twice = splice_injection(&strip_injection_block(&once), &injection);
        // Re-running strip+splice must not accumulate duplicates.
        let count_once = once.matches("android.permission.CAMERA").count();
        let count_twice = twice.matches("android.permission.CAMERA").count();
        assert_eq!(count_once, 1);
        assert_eq!(count_twice, 1);
    }

    #[test]
    fn stripping_restores_baseline() {
        let injection = assemble(&["scanner", "accessibility"]);
        let injected = splice_injection(&strip_injection_block(BASELINE), &injection);
        assert!(injected.contains("SentinelAccessibilityService"));
        let stripped = strip_injection_block(&injected);
        assert!(!stripped.contains("SentinelAccessibilityService"));
        assert!(!stripped.contains("android.permission.CAMERA"));
        assert!(!stripped.contains(INJECTION_MARKER));
        // Core content survives.
        assert!(stripped.contains("SentinelForegroundService"));
    }
}

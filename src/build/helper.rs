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

/// Find the mobile-sentinel crate's `android/` directory (absolute path).
///
/// The crate ships its `android/` tree (the Gradle `core` module + every
/// `caps/<id>` capability module) alongside its source, so the authoritative
/// location is always `CARGO_MANIFEST_DIR/android`. That path is captured at
/// **compile time** via `env!`, which makes it correct in every consumption
/// mode:
///
/// - workspace / path dependency → the in-tree `crates/mobile-sentinel/android`,
/// - published crate from crates.io → the unpacked registry source at
///   `~/.cargo/registry/src/<index>/mobile-sentinel-<ver>/android`.
///
/// The earlier implementation only knew about hard-coded workspace-relative
/// paths and a *runtime* `CARGO_MANIFEST_DIR` (which Cargo does not set for an
/// installed binary), so it panicked the moment the crate was consumed from
/// the registry. An explicit `SENTINEL_ANDROID_DIR` env var still overrides
/// discovery (for vendored copies), and the legacy workspace-relative paths
/// remain as a last-ditch fallback.
fn find_sentinel_android_dir() -> Option<PathBuf> {
    // 1. Explicit override — wins over everything (vendored / relocated trees).
    if let Ok(dir) = std::env::var("SENTINEL_ANDROID_DIR") {
        let path = PathBuf::from(dir);
        if path.exists() {
            return Some(clean_abs(&path));
        }
    }

    // 2. Authoritative: the crate's own bundled android/, resolved from the
    //    compile-time manifest dir. Works for path deps AND registry installs.
    let own = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("android");
    if own.exists() {
        return Some(clean_abs(&own));
    }

    // 3. Legacy workspace-relative fallbacks (older monorepo tooling layouts).
    let candidates = [
        "../../crates/mobile-sentinel/android",
        "../crates/mobile-sentinel/android",
        "crates/mobile-sentinel/android",
    ];
    for candidate in &candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Some(clean_abs(&path));
        }
    }

    None
}

/// Canonicalize a path and strip the Windows `\\?\` verbatim prefix so the
/// result is a clean absolute path Gradle accepts in `projectDir` lines.
fn clean_abs(path: &Path) -> PathBuf {
    match fs::canonicalize(path) {
        Ok(abs) => {
            let abs_str = abs.to_string_lossy().to_string();
            let cleaned = abs_str.strip_prefix(r"\\?\").unwrap_or(&abs_str).to_string();
            PathBuf::from(cleaned)
        }
        Err(_) => path.to_path_buf(),
    }
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
    prepare_android_project_with_capabilities_and_orientation(
        android_project_path,
        activity_fqcn,
        capabilities_declared,
        None,
    )
}

/// Extended form of [`prepare_android_project_with_capabilities`] that also
/// sets a default `android:screenOrientation="..."` attribute on the
/// consumer's main activity (the one named by `activity_fqcn`).
///
/// This is the mechanism for "default behaviour" locking (e.g. "portrait")
/// at build time. Runtime overrides for specific screens (e.g. a settings
/// page that should rotate) are performed via
/// `mobile_sentinel::display::set_requested_orientation`.
///
/// Pass `None` for `default_screen_orientation` to leave the activity's
/// orientation unspecified (system / sensor default, i.e. rotates with phone).
pub fn prepare_android_project_with_capabilities_and_orientation<S: AsRef<str>>(
    android_project_path: &str,
    activity_fqcn: &str,
    capabilities_declared: &[S],
    default_screen_orientation: Option<&str>,
) -> ManifestInjection {
    let project_path = Path::new(android_project_path);

    let sentinel_android = find_sentinel_android_dir().unwrap_or_else(|| {
        panic!(
            "[mobile-sentinel] could not locate the crate's android/ directory \
             (looked at $SENTINEL_ANDROID_DIR, the crate's own \
             CARGO_MANIFEST_DIR/android, and the legacy workspace paths \
             `../../crates/mobile-sentinel/android`, \
             `../crates/mobile-sentinel/android`, `crates/mobile-sentinel/android`). \
             This usually means the crate's bundled android/ tree was excluded \
             from the published package."
        )
    });

    // Forward slashes for Gradle on Windows.
    let sentinel_path_str = sentinel_android.to_string_lossy().replace('\\', "/");

    // Which capability modules to wire in (core is always included).
    let modules = capabilities::enabled_modules(capabilities_declared);

    add_modules_to_settings(project_path, &sentinel_path_str, &modules);
    add_module_dependencies(project_path, &modules);
    add_activity_flags(project_path, activity_fqcn);

    if let Some(orient) = default_screen_orientation {
        set_default_screen_orientation(project_path, activity_fqcn, orient);
    }

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

/// Remove an attribute like `android:foo="bar"` (and its trailing whitespace)
/// from inside an activity (or other) tag string. Used by orientation injection
/// so we can overwrite a previous value when the build config changes.
fn remove_attr(tag: &str, attr_name: &str) -> String {
    let prefix = format!("{}=\"", attr_name);
    let mut result = String::with_capacity(tag.len());
    let mut i = 0usize;
    while i < tag.len() {
        if tag[i..].starts_with(&prefix) {
            // skip until after the closing quote, plus trailing ws
            if let Some(qrel) = tag[i..].find('"') {
                let mut end = i + qrel + 1;
                let bytes = tag.as_bytes();
                while end < bytes.len() && bytes[end].is_ascii_whitespace() {
                    end += 1;
                }
                i = end;
                continue;
            }
        }
        // copy one char (UTF-8 safe? for ascii attrs ok; for full use char indices but attrs are ascii)
        let b = tag.as_bytes()[i];
        result.push(b as char);
        i += 1;
    }
    result
}

/// Insert `attr` (e.g. `android:screenOrientation="portrait"`) immediately
/// before the final `>` of the tag, adding a space if needed.
fn insert_attr_before_close(tag: &str, attr: &str) -> String {
    if let Some(pos) = tag.rfind('>') {
        let (before, close) = tag.split_at(pos);
        // Always need whitespace separator before a new attribute, unless
        // there's already trailing whitespace before the '>'.
        let needs_space = !before.ends_with(char::is_whitespace);
        if needs_space {
            format!("{} {}{}", before, attr, close)
        } else {
            format!("{}{}{}", before, attr, close)
        }
    } else {
        tag.to_string()
    }
}

/// Inject (or overwrite) `android:screenOrientation="..."` on the
/// `<activity android:name="...">` declaration for the main app activity.
/// Idempotent for repeated builds with the same value.
fn set_default_screen_orientation(project_path: &Path, activity_fqcn: &str, orientation: &str) {
    let manifest_path = project_path.join("app/src/main/AndroidManifest.xml");
    let content = match fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let name_needle = format!("android:name=\"{}\"", activity_fqcn);
    let Some(name_pos) = content.find(&name_needle) else {
        eprintln!(
            "[mobile-sentinel] screenOrientation: could not locate activity {} in manifest",
            activity_fqcn
        );
        return;
    };

    // Locate the end of *this* opening tag (first > after the name)
    let rest = &content[name_pos..];
    let Some(rel_close) = rest.find('>') else {
        return;
    };
    let close_abs = name_pos + rel_close;
    let tag = &content[name_pos..=close_abs];

    let desired = format!("android:screenOrientation=\"{}\"", orientation);
    if tag.contains(&desired) {
        return; // already correct
    }

    let cleaned = remove_attr(tag, "android:screenOrientation");
    let updated = insert_attr_before_close(&cleaned, &desired);

    if updated != tag {
        let new_content = format!(
            "{}{}{}",
            &content[..name_pos],
            updated,
            &content[close_abs + 1..]
        );
        match fs::write(&manifest_path, new_content) {
            Ok(()) => eprintln!(
                "[mobile-sentinel] Set android:screenOrientation=\"{}\" on {}",
                orientation, activity_fqcn
            ),
            Err(e) => eprintln!("[mobile-sentinel] Failed to write screenOrientation: {}", e),
        }
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

    #[test]
    fn remove_and_insert_attr_helpers() {
        let tag = r#"<activity android:name="dev.example.Main" android:configChanges="foo" android:exported="true">"#;
        let cleaned = remove_attr(tag, "android:screenOrientation");
        assert_eq!(cleaned, tag); // no-op when absent

        let with_existing = r#"<activity android:name="dev.example.Main" android:screenOrientation="landscape" android:exported="true">"#;
        let cleaned2 = remove_attr(with_existing, "android:screenOrientation");
        assert!(!cleaned2.contains("screenOrientation"));

        let inserted = insert_attr_before_close(&cleaned2, r#"android:screenOrientation="portrait""#);
        assert!(inserted.contains(r#"android:screenOrientation="portrait""#));
        assert!(inserted.contains("android:exported"));

        // Overwrite existing via remove+insert
        let tag3 = r#"<activity android:name="x" android:screenOrientation="foo" >"#;
        let c3 = remove_attr(tag3, "android:screenOrientation");
        let u3 = insert_attr_before_close(&c3, r#"android:screenOrientation="bar""#);
        assert!(u3.contains(r#"="bar""#));
        assert!(!u3.contains(r#"="foo""#));
    }

    #[test]
    fn set_default_screen_orientation_injects_and_overwrites() {
        use std::fs;
        // Simulate the project/app/src/main/ layout that the fn expects.
        let base = std::env::temp_dir().join(format!(
            "sentinel_orient_proj_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let manifest_dir = base.join("app").join("src").join("main");
        fs::create_dir_all(&manifest_dir).unwrap();
        let manifest_path = manifest_dir.join("AndroidManifest.xml");

        let baseline = r#"<?xml version="1.0"?>
<manifest>
    <application>
        <activity android:name="dev.dioxus.main.MainActivity"
            android:configChanges="orientation|screenLayout|screenSize"
            android:exported="true"
            android:launchMode="singleInstance">
        </activity>
    </application>
</manifest>"#;
        fs::write(&manifest_path, baseline).unwrap();

        // Inject portrait (pass the fake *project* root)
        set_default_screen_orientation(&base, "dev.dioxus.main.MainActivity", "portrait");
        let after = fs::read_to_string(&manifest_path).unwrap();
        assert!(after.contains(r#"android:screenOrientation="portrait""#), "after: {}", after);
        assert!(after.contains("configChanges")); // other attrs survive

        // Re-inject different value (landscape) — should overwrite, not duplicate
        set_default_screen_orientation(&base, "dev.dioxus.main.MainActivity", "landscape");
        let after2 = fs::read_to_string(&manifest_path).unwrap();
        assert!(after2.contains(r#"android:screenOrientation="landscape""#));
        assert!(!after2.contains(r#"="portrait""#));
        // only one occurrence
        assert_eq!(after2.matches("screenOrientation").count(), 1);

        // Calling with same is no-op (no change)
        let before_same = fs::read_to_string(&manifest_path).unwrap();
        set_default_screen_orientation(&base, "dev.dioxus.main.MainActivity", "landscape");
        let after_same = fs::read_to_string(&manifest_path).unwrap();
        assert_eq!(before_same, after_same);

        let _ = fs::remove_dir_all(&base);
    }
}

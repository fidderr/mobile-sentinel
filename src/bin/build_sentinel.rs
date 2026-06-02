//! build_sentinel — Wire mobile-sentinel's Kotlin module, copy assets/icon, build APK/AAB.
//!
//! # Usage (via the published crate, as used by consumers' build.sh)
//!
//! ```bash
//! build_sentinel
//! build_sentinel --release
//! build_sentinel --release --aab
//! build_sentinel --app alarmfree --release --aab
//! ```
//!
//! For Google Play: use `--release --aab` (from your final build script) to produce
//! an AAB under the package name from the consumer's Dioxus.toml `[bundle] identifier`
//! (or `[android] identifier`).
//!
//! The `build.sh` script in the AlarmFree tree is the blessed way to do final
//! (including release/AAB) builds because it exercises the published crate.
//!
//! # Configuration (`sentinel.toml`)
//!
//! ```toml
//! [android]
//! activity = "com.example.MainActivity"  # required (or pass --activity)
//! icon = "path/to/icon.webp"             # optional — copies to all mipmap densities
//! assets = ["path/to/sounds", "other/dir"]   # optional — copies to APK assets/
//! screen_orientation = "portrait"        # optional — sets android:screenOrientation on main activity (build-time default lock)
//! ```
//!
//! Trimming is structural and unconditional: only the capability modules a
//! consumer's enabled features select are wired into the Gradle build, so a
//! disabled capability's Kotlin/deps are never compiled (there is no opt-out).
//!
//! Capabilities are NOT declared in `sentinel.toml`. They are derived from
//! the Cargo features the consumer compiled mobile-sentinel with (the single
//! compile-enforced source of truth); `build_sentinel` reads the enabled set
//! and injects exactly those permissions + components into the merged
//! manifest.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let config = load_config();

    println!("=== build_sentinel ===");
    println!("  Project: {}", config.project_path.display());
    println!("  Activity: {}", config.activity);
    println!();

    // Step 0: wipe stale Gradle module output so a previous run's modules
    // (e.g. a different app's, or a prior feature set's) never linger in the
    // build. Every module redirects its buildDir under the consumer-side
    // build root (see `sentinel_build_root`), so one wipe clears all module
    // output.
    clean_module_builds(&config.project_path);

    // Step 1: Wire mobile-sentinel Kotlin module + manifest attributes +
    // capability-gated permissions/components.
    println!("[1/3] Wiring mobile-sentinel...");
    if config.capabilities.is_empty() {
        println!("  Capabilities: none (baseline manifest only)");
    } else {
        println!("  Capabilities: {}", config.capabilities.join(", "));
    }
    let default_orient = config.default_screen_orientation.as_deref();
    let injection = mobile_sentinel::prepare_android_project_with_capabilities_and_orientation(
        config.project_path.to_str().unwrap(),
        &config.activity,
        &config.capabilities,
        default_orient,
    );
    if !injection.unknown.is_empty() {
        eprintln!(
            "  WARNING: unknown capabilities ignored: {}",
            injection.unknown.join(", ")
        );
        eprintln!("  (valid ids — see `mobile_sentinel::CAPABILITIES`)");
    }
    for warning in &injection.policy_warnings {
        eprintln!("  ⚠ POLICY: {}", warning);
    }
    if !injection.policy_warnings.is_empty() {
        eprintln!("  ⚠ These capabilities are flagged by Google Play review. Ship only if you have a documented justification.");
    }

    // Step 2: Copy icon + assets (if configured)
    println!("[2/3] Assets...");
    if let Some(icon) = &config.icon_path {
        copy_icon(icon, &config.project_path);
    }
    copy_assets(&config.asset_paths, &config.project_path);

    // Step 3: Build APK/AAB via Gradle. Trimming is now STRUCTURAL — only the
    // enabled capability modules were wired into settings.gradle + the app
    // dependencies (see prepare_android_project_with_capabilities), so a
    // disabled capability's Kotlin/deps are never compiled. No per-build
    // exclusion properties needed.
    let task = if config.bundle_aab {
        ":app:bundleRelease"
    } else if config.release {
        ":app:assembleRelease"
    } else {
        ":app:assembleDebug"
    };
    let artifact_label = if config.bundle_aab {
        "AAB"
    } else if config.release {
        "release APK"
    } else {
        "APK"
    };

    println!("[3/3] Building {} via {}...", artifact_label, task);
    let modules = mobile_sentinel::build::registry::enabled_modules(&config.capabilities);
    println!(
        "  Modules: :sentinel-core + {} capability module(s){}",
        modules.len(),
        if modules.is_empty() {
            String::new()
        } else {
            format!(" ({})", modules.join(", "))
        }
    );
    run_gradle(
        &config.project_path,
        &[sentinel_build_root_arg(&config.project_path)],
        task,
    );

    let out_path = if config.bundle_aab {
        config
            .project_path
            .join("app/build/outputs/bundle/release/app-release.aab")
    } else if config.release {
        // Note: may be -unsigned if no signing config provided to Gradle.
        config
            .project_path
            .join("app/build/outputs/apk/release/app-release-unsigned.apk")
    } else {
        config
            .project_path
            .join("app/build/outputs/apk/debug/app-debug.apk")
    };
    println!();
    println!("=== {} ready: {} ===", artifact_label, out_path.display());
}

struct Config {
    project_path: PathBuf,
    activity: String,
    icon_path: Option<PathBuf>,
    asset_paths: Vec<PathBuf>,
    capabilities: Vec<String>,
    default_screen_orientation: Option<String>,
    release: bool,
    bundle_aab: bool,
}

fn load_config() -> Config {
    let toml = read_sentinel_toml();

    let release = parse_release_cli();
    let bundle_aab = parse_aab_cli();

    let activity = parse_activity_cli()
        .or_else(|| toml.get("activity").cloned())
        .or_else(|| find_project_path(release).and_then(|p| detect_activity_from_manifest(&p)))
        .expect("Activity not specified. Use --activity or set in sentinel.toml");

    let project_path = find_project_path(release)
        .expect("Could not find Android project. Run `dx build --platform android [--release]` first.");

    let icon_path = toml.get("icon").map(PathBuf::from).filter(|p| p.exists());

    let asset_paths: Vec<PathBuf> = toml
        .get("assets")
        .map(|s| parse_string_list(s))
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect();

    let default_screen_orientation = toml.get("screen_orientation").cloned();

    // Capability resolution.
    //
    // When `--app <name>` is given (multi-consumer workspace), derive the
    // capability set DETERMINISTICALLY from that app's `Cargo.toml`
    // mobile-sentinel feature list — the true source of truth, immune to the
    // build-script-output-timestamp contamination that can occur when two
    // consumers share the cargo target dir.
    //
    // Without `--app` (single consumer), fall back to reading the capability
    // set the compiled `.so`'s build.rs recorded (`enabled_capabilities.txt`).
    let capabilities = match parse_app_cli() {
        Some(app) => {
            let caps = read_app_cargo_capabilities(&app);
            if !caps.is_empty() {
                eprintln!(
                    "  Feature-derived capabilities (from {app}/Cargo.toml or similar): {}",
                    caps.join(", ")
                );
            } else {
                eprintln!(
                    "  No mobile-sentinel features declared by {app} (searched apps/{app}/ etc) — kernel-only build"
                );
            }
            caps
        }
        None => {
            let caps = read_feature_capabilities();
            if !caps.is_empty() {
                eprintln!(
                    "  Feature-derived capabilities (from compiled .so): {}",
                    caps.join(", ")
                );
            }
            caps
        }
    };

    Config {
        project_path,
        activity,
        icon_path,
        asset_paths,
        capabilities,
        default_screen_orientation,
        release,
        bundle_aab,
    }
}

/// Discover the capabilities derived from the Cargo features the consumer's
/// mobile-sentinel `.so` was compiled with. mobile-sentinel's `build.rs`
/// writes the enabled capability ids to `enabled_capabilities.txt` in its
/// OUT_DIR on every build.
///
/// IMPORTANT: we must read the capability set belonging to the **Android
/// `.so` that is actually bundled into the APK** — and ONLY that one. Cargo
/// keeps stale build-script output directories from previous builds (and from
/// host/other-profile builds with a different feature set). Reading the wrong
/// one would inject permissions/components/modules the shipped `.so` does not
/// contain — exactly the bloat this whole system exists to prevent.
///
/// So we scope tightly to the `.so` `dx build` actually produced:
///   1. Only Android target-triple dirs (name contains `android`, e.g.
///      `aarch64-linux-android`) — excludes host `debug`/`release`/`doc`.
///   2. WITHIN those, only the `android-dev` profile dir — the profile `dx`
///      compiles under. This is the key fix: a stray
///      `cargo build --target aarch64-linux-android --features ...` writes to
///      the SAME triple but under the `debug` profile, and unioning/picking
///      across profiles would read that polluted feature set.
///   3. Among the remaining files, pick the SINGLE most-recently-modified one
///      — the `.so` from the `dx build` that just ran. No union.
///
/// Returns an empty vec if none are found (e.g. before any Android build, or
/// when no code-bearing capability features are enabled — the husk case).
fn read_feature_capabilities() -> Vec<String> {
    let target = Path::new("target");
    if !target.exists() {
        return Vec::new();
    }

    // Collect candidate files under each Android target-triple's `android-dev`
    // profile dir (the profile `dx build` uses). Falls back to scanning the
    // whole triple only if no `android-dev` dir exists (older dx layouts).
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(target) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_android_triple = path.is_dir()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.contains("android"))
                    .unwrap_or(false);
            if !is_android_triple {
                continue;
            }
            let dx_profile = path.join("android-dev");
            if dx_profile.is_dir() {
                collect_capability_files(&dx_profile, &mut candidates, 0);
            } else {
                // Older dx layout: no separate profile dir under the triple.
                collect_capability_files(&path, &mut candidates, 0);
            }
        }
    }

    // Pick the single freshest file (the current build's .so), not a union
    // across stale feature-combo build dirs.
    let freshest = candidates
        .into_iter()
        .max_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok());

    let mut found: Vec<String> = Vec::new();
    if let Some(path) = freshest {
        if let Ok(contents) = fs::read_to_string(&path) {
            for line in contents.lines() {
                let id = line.trim();
                if !id.is_empty() {
                    found.push(id.to_string());
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Recursively collect paths to `enabled_capabilities.txt` (depth-limited to
/// avoid scanning the entire tree). The file lives at
/// `target/<triple>/<profile>/build/mobile-sentinel-<hash>/out/`.
fn collect_capability_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 6 {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_capability_files(&path, out, depth + 1);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("enabled_capabilities.txt") {
            out.push(path);
        }
    }
}

/// Derive the capability ids a consumer enables by parsing its
/// Cargo.toml (searched in apps/<app>/, <app>/, ./ , ../ etc to support
/// different workspace layouts) mobile-sentinel dependency `features = [...]`.
///
/// This is the deterministic capability source for `--app` builds: it reads
/// exactly what the app declares, independent of any build-script output
/// timestamps (which can be contaminated when two consumers share the cargo
/// target dir). The feature → capability-id mapping (and the `alarm-kit` /
/// `firing` bundle expansion) lives in `registry::capabilities_for_features`.
fn read_app_cargo_capabilities(app: &str) -> Vec<String> {
    // Try several likely locations to support both "monorepo with apps/<name>/"
    // layouts and flat sibling layouts (e.g. <name>/ + mobile-sentinel/ at same level),
    // and invocation from either the workspace root or from inside the app dir.
    let candidates = [
        format!("apps/{app}/Cargo.toml"),
        format!("{app}/Cargo.toml"),
        "Cargo.toml".to_string(),
        format!("../{app}/Cargo.toml"),
    ];
    for p in &candidates {
        let path = PathBuf::from(p);
        if let Ok(content) = fs::read_to_string(&path) {
            let features = parse_mobile_sentinel_features(&content);
            if !features.is_empty() {
                return mobile_sentinel::build::registry::capabilities_for_features(&features);
            }
            // continue; an empty features list might be from a different toml
        }
    }
    Vec::new()
}

/// Extract the `features = [...]` list from the `mobile-sentinel` dependency
/// line(s) of a consumer `Cargo.toml`. Handles the common inline-table form
/// `mobile-sentinel = { path = "...", features = ["a", "b"] }` (the form dx
/// apps use). Returns an empty vec when the dep has no features (the husk).
fn parse_mobile_sentinel_features(cargo_toml: &str) -> Vec<String> {
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        // Match the dependency key `mobile-sentinel` (ignore dev-dep duplicates
        // — they declare the same feature set, so first match wins).
        if !(trimmed.starts_with("mobile-sentinel ") || trimmed.starts_with("mobile-sentinel=")) {
            continue;
        }
        if let Some(start) = trimmed.find("features") {
            // Find the bracketed list after `features`.
            if let Some(open) = trimmed[start..].find('[') {
                let abs_open = start + open;
                if let Some(close) = trimmed[abs_open..].find(']') {
                    let inner = &trimmed[abs_open + 1..abs_open + close];
                    return inner
                        .split(',')
                        .map(|s| s.trim().trim_matches('"').to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
        }
        // mobile-sentinel dep present but no features → kernel-only.
        return Vec::new();
    }
    Vec::new()
}

fn parse_activity_cli() -> Option<String> {
    parse_cli_value("--activity")
}

/// Parse a `--flag value` or `--flag=value` CLI argument.
fn parse_cli_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let eq_prefix = format!("{flag}=");
    for (i, arg) in args.iter().enumerate() {
        if arg == flag {
            return args.get(i + 1).cloned();
        }
        if let Some(val) = arg.strip_prefix(&eq_prefix) {
            return Some(val.to_string());
        }
    }
    None
}

/// The consumer app to build for, from `--app <name>`. When set, the dx
/// project, `sentinel.toml`, and (implicitly) the freshest capability file are
/// scoped to that app — required when the workspace has more than one
/// mobile-sentinel consumer sharing the cargo target dir.
fn parse_app_cli() -> Option<String> {
    parse_cli_value("--app")
}

fn parse_release_cli() -> bool {
    let args: Vec<String> = std::env::args().collect();
    args.iter().any(|a| a == "--release" || a == "-r")
}

fn parse_aab_cli() -> bool {
    let args: Vec<String> = std::env::args().collect();
    args.iter().any(|a| a == "--aab" || a == "--bundle")
}

fn parse_string_list(s: &str) -> Vec<String> {
    let trimmed = s.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        trimmed[1..trimmed.len() - 1]
            .split(',')
            .map(|item| item.trim().trim_matches('"').to_string())
            .filter(|item| !item.is_empty())
            .collect()
    } else {
        vec![trimmed.to_string()]
    }
}

fn read_sentinel_toml() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    // When `--app <name>` is given, read ONLY that app's sentinel.toml so a
    // multi-consumer workspace doesn't pick the wrong app's config.
    let mut search_paths: Vec<PathBuf> = match parse_app_cli() {
        Some(app) => vec![
            PathBuf::from(format!("apps/{app}/sentinel.toml")),
            PathBuf::from("sentinel.toml"),
        ],
        None => {
            let mut paths = vec![PathBuf::from("sentinel.toml")];
            if let Ok(entries) = fs::read_dir("apps") {
                for entry in entries.flatten() {
                    paths.push(entry.path().join("sentinel.toml"));
                }
            }
            paths
        }
    };
    // Dedup while preserving order.
    search_paths.dedup();
    for path in &search_paths {
        if let Ok(content) = fs::read_to_string(path) {
            let mut section = "";
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    section = match trimmed {
                        "[android]" => "android",
                        _ => "",
                    };
                    continue;
                }
                if let Some((key, val)) = trimmed.split_once('=') {
                    let key = key.trim();
                    let val = val.trim();
                    if section == "android" {
                        let v = val.trim_matches('"').to_string();
                        if !v.is_empty() {
                            map.insert(key.to_string(), v);
                        }
                    }
                }
            }
            if !map.is_empty() {
                eprintln!("  Using config: {}", path.display());
                return map;
            }
        }
    }
    map
}

/// The writable directory every mobile-sentinel Gradle module redirects its
/// build output into, located inside the consumer's Android project (which is
/// always writable, unlike the crate's own `android/` tree when consumed from
/// crates.io). Passed to Gradle as `-PsentinelBuildRoot=<dir>`; each module's
/// `build.gradle.kts` reads it and falls back to its in-tree `android/builds/`
/// path only for workspace/path-dependency development.
fn sentinel_build_root(project_path: &Path) -> PathBuf {
    project_path.join("sentinel-build")
}

/// The `-PsentinelBuildRoot=<abs dir>` Gradle argument. Uses forward slashes
/// so the value is valid in Gradle on Windows.
fn sentinel_build_root_arg(project_path: &Path) -> String {
    let root = sentinel_build_root(project_path);
    let s = root.to_string_lossy().replace('\\', "/");
    format!("-PsentinelBuildRoot={s}")
}

/// Wipe the centralized Gradle module build output so each `build_sentinel`
/// run starts clean. Every module redirects its `buildDirectory` under the
/// consumer-side build root (`<project>/sentinel-build`), so removing that one
/// folder clears all module output — preventing a prior run's modules (a
/// different app's, or a prior feature set's) from lingering. Best-effort: a
/// failure is non-fatal (Gradle would just recompile).
fn clean_module_builds(project_path: &Path) {
    let root = sentinel_build_root(project_path);
    if root.exists() {
        match fs::remove_dir_all(&root) {
            Ok(()) => println!("  Cleaned stale module builds: {}", root.display()),
            Err(e) => eprintln!("  (warn) could not clean {}: {e}", root.display()),
        }
    }
}

fn find_project_path(is_release: bool) -> Option<PathBuf> {
    let dx_dir = Path::new("target/dx");
    if !dx_dir.exists() {
        return None;
    }

    let profile = if is_release { "release" } else { "debug" };

    // When `--app <name>` is given, build that specific consumer's project.
    if let Some(app) = parse_app_cli() {
        let candidate = dx_dir.join(&app).join(format!("{}/android/app", profile));
        if candidate.exists() {
            return Some(candidate);
        }
        eprintln!(
            "[mobile-sentinel] --app {app} given but {} not found; run `dx build --platform android {}` for it first",
            candidate.display(),
            if is_release { "--release" } else { "" }
        );
        return None;
    }

    // Otherwise pick the most-recently-built dx project (the one whose
    // android/app was generated last). This keeps single-consumer builds
    // working and, for a back-to-back `dx build && build_sentinel`, selects
    // the app that was just built even if others exist.
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for entry in fs::read_dir(dx_dir).ok()?.flatten() {
        let candidate = entry.path().join(format!("{}/android/app", profile));
        if !candidate.exists() {
            continue;
        }
        let mtime = fs::metadata(&candidate)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        match &best {
            Some((_, best_mtime)) if *best_mtime >= mtime => {}
            _ => best = Some((candidate, mtime)),
        }
    }
    best.map(|(p, _)| p)
}

fn detect_activity_from_manifest(project: &Path) -> Option<String> {
    let manifest = project.join("app/src/main/AndroidManifest.xml");
    let content = fs::read_to_string(manifest).ok()?;
    for line in content.lines() {
        if line.contains("<activity") && line.contains("android:name=") {
            let start = line.find("android:name=\"")? + 14;
            let end = line[start..].find('"')? + start;
            return Some(line[start..end].to_string());
        }
    }
    None
}

fn copy_icon(icon_source: &Path, project_path: &Path) {
    let res_path = project_path.join("app/src/main/res");
    let densities = [
        "mipmap-mdpi",
        "mipmap-hdpi",
        "mipmap-xhdpi",
        "mipmap-xxhdpi",
        "mipmap-xxxhdpi",
    ];

    // Determine extension
    let ext = icon_source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("webp");
    let target_name = format!("ic_launcher.{}", ext);

    for dir in &densities {
        let target_dir = res_path.join(dir);
        let _ = fs::create_dir_all(&target_dir);
        let _ = fs::copy(icon_source, target_dir.join(&target_name));
        // Remove conflicting other format
        if ext == "webp" {
            let _ = fs::remove_file(target_dir.join("ic_launcher.png"));
        } else {
            let _ = fs::remove_file(target_dir.join("ic_launcher.webp"));
        }
        // Remove round variants that might conflict
        let _ = fs::remove_file(target_dir.join("ic_launcher_round.png"));
        let _ = fs::remove_file(target_dir.join("ic_launcher_round.webp"));
    }

    // Remove adaptive icon XML that overrides our raster icon
    let _ = fs::remove_file(res_path.join("mipmap-anydpi-v26/ic_launcher.xml"));
    let _ = fs::remove_file(res_path.join("mipmap-anydpi-v26/ic_launcher_round.xml"));
    let _ = fs::remove_file(res_path.join("drawable/ic_launcher_background.xml"));
    let _ = fs::remove_file(res_path.join("drawable-v24/ic_launcher_foreground.xml"));

    eprintln!("  Icon: {}", icon_source.display());
}

fn copy_assets(asset_paths: &[PathBuf], project_path: &Path) {
    if asset_paths.is_empty() {
        eprintln!("  No assets configured");
        return;
    }
    let dest_base = project_path.join("app/src/main/assets");
    let _ = fs::create_dir_all(&dest_base);
    let mut total = 0;
    for path in asset_paths {
        if path.is_file() {
            let _ = fs::copy(path, dest_base.join(path.file_name().unwrap()));
            total += 1;
        } else if path.is_dir() {
            total += copy_dir_recursive(path, &dest_base, path);
        }
    }
    eprintln!("  Copied {} asset files", total);
}

fn copy_dir_recursive(src: &Path, dest_base: &Path, root: &Path) -> usize {
    let mut count = 0;
    let Ok(entries) = fs::read_dir(src) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count += copy_dir_recursive(&path, dest_base, root);
        } else if path.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let dest = dest_base.join(relative);
            if let Some(parent) = dest.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::copy(&path, &dest);
            count += 1;
        }
    }
    count
}

fn run_gradle(project_path: &Path, extra_props: &[String], task: &str) {
    let ms_build = project_path.join("mobile-sentinel/build");
    if ms_build.exists() {
        let _ = fs::remove_dir_all(&ms_build);
    }

    let gradlew = if cfg!(windows) {
        project_path.join("gradlew.bat")
    } else {
        project_path.join("gradlew")
    };

    let mut cmd = Command::new(&gradlew);
    cmd.arg(task)
        .arg("--no-daemon")
        .arg("--no-build-cache");
    for prop in extra_props {
        cmd.arg(prop);
    }
    let status = cmd
        .current_dir(project_path)
        .status()
        .expect("Failed to run Gradle");

    if !status.success() {
        eprintln!("  BUILD FAILED");
        std::process::exit(1);
    }
    eprintln!("  Done");
}

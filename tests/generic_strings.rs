//! Property 5 enforcement test — `mobile-sentinel` is generic.
//! **Property 5: Preservation** — The crate MUST NOT contain alarm-specific
//! or AlarmFree-specific strings anywhere in its published surface.
//! AlarmFree supplies all app-specific values (activity FQCN, channel IDs, and
//! notification content) via config structs; the crate defaults stay app-neutral.
//! This test walks every `.rs`, `.kt`, `.xml`, `.json` file under
//! `crates/mobile-sentinel/` and fails if any file contains a forbidden
//! substring outside of an explicit whitelist.
//! **Validates: Requirements 22.1, 22.3, 22.4, 25.2**
//! _Properties: P5, P15_

use std::fs;
use std::path::{Path, PathBuf};

/// Substrings that MUST NOT appear anywhere in the crate.
/// These are consumer-app-specific identifiers. Generic platform concepts
/// (e.g. `ScheduleAlarm` as an Action variant, `Trigger::Fire`) are NOT
/// forbidden — they are part of the universal orchestration vocabulary.
/// `kiosk` is NOT forbidden (Req 22.3).
const FORBIDDEN: &[&str] = &[
    "alarmfree",
    "AlarmFree",
    "dev.dioxus.main",
    "dev.dioxus",
    "com.example.Alarmfree",
];

/// File-path suffixes treated as source-of-truth.
const SCAN_EXTENSIONS: &[&str] = &[".rs", ".kt", ".xml", ".json"];

/// Paths (relative to the crate root) to exclude from the scan. These are
/// generated or third-party artifacts that cannot introduce a regression.
const EXCLUDE_PATH_SEGMENTS: &[&str] = &[
    "/target/",
    "/.git/",
    // Gradle build-output directories (generated artifacts that embed the
    // absolute project path, which legitimately contains the consumer's
    // workspace name). Covers the per-module split: the centralized
    // android/builds/ output dir, plus any stray module-local build dir.
    "/builds/",
    "/build/",
    "/android/.gradle/",
    "/.gradle/",
    "/node_modules/",
    // Documentation may legitimately reference AlarmFree as an example
    // consumer. Keep doc files out of this test — the contract is about
    // the compiled surface, not narrative prose.
    "/docs/",
];

/// Exception: `tests/fixtures/canonical_json/**/*.json` is exempt from the
/// scan. The fixture files are the single source of truth for the
/// cross-platform wire format and must mirror real-world payloads, including
/// legitimate platform constants such as `"usage":"alarm"` (Android
/// `AudioAttributes.USAGE_ALARM`) and `"source":"system:default-alarm"`.
/// These are vendor strings, not consumer-app vocabulary, so they do not
/// violate Req 22's intent.
const EXEMPT_PATH_SEGMENTS: &[&str] = &[];

/// (file relative path, line pattern): permitted exceptions where the
/// forbidden substring appears in a way that doesn't violate the intent
/// of Property 5.
/// Empty by default — ALL occurrences of a forbidden substring are
/// violations unless explicitly whitelisted here.
const WHITELIST: &[(&str, &str)] = &[];

fn crate_root() -> PathBuf {
    let raw = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(raw)
}

fn is_scannable(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    SCAN_EXTENSIONS.iter().any(|ext| name.ends_with(ext))
}

fn is_excluded(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    EXCLUDE_PATH_SEGMENTS.iter().any(|seg| s.contains(seg))
}

fn is_exempt(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    EXEMPT_PATH_SEGMENTS.iter().any(|seg| s.contains(seg))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_excluded(&path) {
            continue;
        }
        if is_exempt(&path) {
            continue;
        }
        if path.is_dir() {
            walk(&path, out);
        } else if is_scannable(&path) {
            out.push(path);
        }
    }
}

fn is_whitelisted(rel_path: &str, line: &str) -> bool {
    WHITELIST
        .iter()
        .any(|(p, l)| rel_path.contains(p) && line.contains(l))
}

#[test]
fn mobile_sentinel_is_generic() {
    let root = crate_root();
    let mut files = Vec::new();
    walk(&root, &mut files);

    assert!(
        !files.is_empty(),
        "generic_strings test could not find any source files under {}",
        root.display(),
    );

    // We also need to exclude this test file itself — it contains every
    // forbidden substring by definition (the FORBIDDEN constant).
    let this_file = "tests/generic_strings.rs".replace('/', std::path::MAIN_SEPARATOR_STR);

    let mut violations: Vec<String> = Vec::new();

    for file in &files {
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .to_string();

        if rel.ends_with(&this_file) {
            continue;
        }

        let contents = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for (line_no, line) in contents.lines().enumerate() {
            for forbidden in FORBIDDEN {
                if line.contains(forbidden) {
                    if is_whitelisted(&rel, line) {
                        continue;
                    }
                    violations.push(format!(
                        "{}:{} contains '{}': {}",
                        rel,
                        line_no + 1,
                        forbidden,
                        line.trim(),
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Property 5 violated — mobile-sentinel contains app-specific strings:\n{}",
        violations.join("\n"),
    );
}

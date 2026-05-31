//! Manifest process-topology test.
//!
//! Asserts the correct process assignment for each component:
//! - `:sentinel` process: alarm receiver, time-change receiver, job guardian
//! - MAIN process (default): foreground service, boot service, kiosk init
//!
//! The architecture is:
//! - `:sentinel` = job guardian only (polls files, restarts MAIN)
//! - MAIN = all logic (audio, kiosk, FGS, Rust state machine)

use std::fs;
use std::path::{Path, PathBuf};

fn manifest_path() -> PathBuf {
    exact_alarm_manifest_path()
}

/// The exact-alarm components (alarm/time/boot receivers, boot service) live
/// in the exact_alarm module's manifest — there is no shared "alarm runtime"
/// bundle anymore.
fn exact_alarm_manifest_path() -> PathBuf {
    cap_manifest_path("exact_alarm")
}

/// The foreground service lives in its own foreground_service module.
fn foreground_service_manifest_path() -> PathBuf {
    cap_manifest_path("foreground_service")
}

fn cap_manifest_path(cap: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("android")
        .join("caps")
        .join(cap)
        .join("src")
        .join("main")
        .join("AndroidManifest.xml")
}

/// Parse every `<receiver>` / `<service>` / `<provider>` element name +
/// process attribute. Minimal hand-rolled parser.
fn extract_components(xml: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for elem in ["<receiver", "<service", "<provider"] {
        let mut start = 0;
        while let Some(pos) = xml[start..].find(elem) {
            let abs = start + pos;
            let end = xml[abs..].find('>').map(|e| abs + e).unwrap_or(xml.len());
            let header = &xml[abs..=end];
            let name = extract_attr(header, "android:name");
            let process = extract_attr(header, "android:process");
            if let Some(n) = name {
                out.push((n, process));
            }
            start = end + 1;
        }
    }
    out
}

fn extract_attr(header: &str, key: &str) -> Option<String> {
    let needle = format!("{}=\"", key);
    let i = header.find(&needle)?;
    let after = &header[i + needle.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_owned())
}

#[test]
fn sentinel_process_components() {
    let xml = fs::read_to_string(manifest_path()).expect("read manifest");
    let components = extract_components(&xml);

    // Match a component by its simple class name, tolerating either a
    // relative (`.SentinelAlarmReceiver`) or fully-qualified
    // (`com.mobilesentinel.SentinelAlarmReceiver`) `android:name`. The
    // exact_alarm manifest declares components fully-qualified (its Gradle
    // namespace differs from the classes' `com.mobilesentinel` package, so a
    // relative name would resolve to a non-existent class — see the manifest).
    let lookup = |simple_name: &str| -> Option<Option<String>> {
        components.iter().find_map(|(n, p)| {
            let matches =
                n == simple_name || n.rsplit('.').next() == simple_name.rsplit('.').next();
            if matches {
                Some(p.clone())
            } else {
                None
            }
        })
    };

    // These run in :sentinel (the job guardian process)
    for required in [".SentinelAlarmReceiver", ".SentinelTimeChangeReceiver"] {
        let process =
            lookup(required).unwrap_or_else(|| panic!("manifest missing component {required}"));
        assert_eq!(
            process.as_deref(),
            Some(":sentinel"),
            "{required} must declare android:process=\":sentinel\""
        );
    }

    // FGS runs in MAIN process (no android:process attribute = default). It
    // lives in the foreground_service module's manifest.
    let fgs_xml = fs::read_to_string(foreground_service_manifest_path())
        .expect("read foreground_service manifest");
    let fgs_components = extract_components(&fgs_xml);
    let fgs_process = fgs_components
        .iter()
        .find_map(|(n, p)| {
            if n.rsplit('.').next() == Some("SentinelForegroundService") {
                Some(p.clone())
            } else {
                None
            }
        })
        .expect("foreground_service manifest missing SentinelForegroundService");
    assert_eq!(
        fgs_process, None,
        "SentinelForegroundService must run in MAIN process (no android:process)"
    );
}

/// Legacy components must be gone.
#[test]
fn legacy_components_are_removed() {
    let xml = fs::read_to_string(manifest_path()).expect("read manifest");
    for obsolete in [
        "SentinelTriggerReceiver",
        "SentinelWatchdogReceiver",
        "SentinelPlanNotifier",
        "SentinelPlanSentinelReceiver",
        "SentinelFireWatchdog",
        "SentinelFiringNotifier",
        "SentinelContentProvider",
    ] {
        assert!(
            !xml.contains(obsolete),
            "manifest still references obsolete component `{obsolete}`"
        );
    }
}

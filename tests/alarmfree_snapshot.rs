//! An app that declares nothing renders the manifest it always did.
//! The fixture is a copy of AlarmFree's sentinel.toml; the snapshot was taken before the
//! declaration layer existed.

use mobile_sentinel::build::host::{render_native_manifest, HostConfig};
use std::collections::HashMap;
use std::path::Path;

fn flat_android_keys(toml: &str) -> HashMap<String, String> {
    let mut keys = HashMap::new();
    let mut in_android = false;
    for line in toml.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_android = line == "[android]";
        } else if let (true, Some((k, v))) = (in_android, line.split_once('=')) {
            keys.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
        }
    }
    keys
}

#[test]
fn alarmfree_manifest_is_unchanged() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let keys =
        flat_android_keys(&std::fs::read_to_string(dir.join("alarmfree-sentinel.toml")).unwrap());
    let cfg = HostConfig::from_toml(&keys, Path::new(".")).unwrap();
    let manifest = render_native_manifest(&cfg, cfg.native_lib.as_deref().unwrap());
    let snapshot = dir.join("alarmfree-native-manifest.xml");
    if std::env::var("UPDATE_SNAPSHOT").is_ok() {
        std::fs::write(&snapshot, &manifest).unwrap();
    }
    let expected = std::fs::read_to_string(snapshot)
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(manifest, expected);
}

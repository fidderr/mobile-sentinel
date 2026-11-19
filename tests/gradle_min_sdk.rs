//! Every Kotlin module takes its `minSdk` from the app (`-PsentinelMinSdk`), so an app with an
//! older minimum is not refused by the manifest merger. A hard-coded number would undo that.

use std::fs;
use std::path::Path;

const EXPRESSION: &str = r#"(project.findProperty("sentinelMinSdk") as String?)?.toInt() ?: 31"#;

fn module_builds() -> Vec<std::path::PathBuf> {
    let android = Path::new(env!("CARGO_MANIFEST_DIR")).join("android");
    let mut builds = vec![android.join("core/build.gradle.kts")];
    for entry in fs::read_dir(android.join("caps")).unwrap() {
        let file = entry.unwrap().path().join("build.gradle.kts");
        if file.exists() {
            builds.push(file);
        }
    }
    builds
}

#[test]
fn every_module_reads_the_min_sdk_from_the_app() {
    let builds = module_builds();
    assert!(builds.len() > 40);
    for file in builds {
        let text = fs::read_to_string(&file).unwrap();
        let line = text
            .lines()
            .find(|l| l.contains("minSdk ="))
            .unwrap_or_else(|| panic!("{} sets no minSdk", file.display()));
        assert!(
            line.contains(EXPRESSION),
            "{} hard-codes its minSdk: {line}",
            file.display()
        );
    }
}

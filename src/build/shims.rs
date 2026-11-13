//! One-line Java classes for the components of an app whose code is Rust.
//!
//! Android creates services, receivers and activities by the class names in the manifest.
//! For each component of the app's own package that none of its sources defines, the
//! generated project gets a subclass of an SDK base (`com.mobilesentinel.rust.*`) that
//! hands every lifecycle event to the native library handling the component:
//!
//! - an activity with `android.app.lib_name` meta-data: `android.app.NativeActivity`
//!   (a screen drawn by that library; nothing to forward), or `RustNativeActivity` when it
//!   also names a `mobilesentinel.lib` (then its results and new intents are forwarded);
//! - any other activity: `RustActivity`; a service: `RustService`;
//! - a receiver with `android.app.device_admin` meta-data: `RustAdminReceiver`, any other
//!   receiver: `RustReceiver`.
//!
//! The library is the component's `<meta-data android:name="mobilesentinel.lib"
//! android:value="name"/>`, or `rust_lib` of sentinel.toml. For each library used there is
//! a bridge class `com.mobilesentinel.rust.Lib_<name>` that loads it and declares
//!
//! ```text
//! static native Object event(String component, String event, Object self, Object intent, int a, int b)
//! ```
//!
//! so the library exports `Java_com_mobilesentinel_rust_Lib_1<name>_event` (with
//! `mobile_sentinel::rust_components!` or by hand). A library name is letters and digits.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    Native,
    NativeRust,
    Activity,
    Service,
    Receiver,
    AdminReceiver,
}

impl Base {
    fn class(self) -> &'static str {
        match self {
            Base::Native => "android.app.NativeActivity",
            Base::NativeRust => "com.mobilesentinel.rust.RustNativeActivity",
            Base::Activity => "com.mobilesentinel.rust.RustActivity",
            Base::Service => "com.mobilesentinel.rust.RustService",
            Base::Receiver => "com.mobilesentinel.rust.RustReceiver",
            Base::AdminReceiver => "com.mobilesentinel.rust.RustAdminReceiver",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// Fully qualified class name.
    pub class: String,
    pub base: Base,
    /// The `mobilesentinel.lib` meta-data of the component, if any.
    pub lib: Option<String>,
}

/// The app's components in `manifest`, with names resolved against `namespace`.
/// Only names in the app's own package count: a library's components bring their class.
pub fn components(manifest: &str, namespace: &str) -> Vec<Component> {
    let mut out = Vec::new();
    for (tag, base) in [
        ("activity", Base::Activity),
        ("service", Base::Service),
        ("receiver", Base::Receiver),
    ] {
        let open = format!("<{tag}");
        let mut rest = manifest;
        while let Some(at) = rest.find(&open) {
            let after = &rest[at + open.len()..];
            // `<activity-alias` and the like are other tags.
            if !after.starts_with(|c: char| c.is_whitespace()) {
                rest = after;
                continue;
            }
            let Some(head_end) = after.find('>') else {
                break;
            };
            let head = &after[..head_end];
            let body = if head.ends_with('/') {
                ""
            } else {
                let close = format!("</{tag}>");
                let tail = &after[head_end + 1..];
                &tail[..tail.find(&close).unwrap_or(tail.len())]
            };
            rest = &after[head_end..];
            let Some(name) = attr(head, "android:name") else {
                continue;
            };
            let class = resolve(&name, namespace);
            if !class.starts_with(&format!("{namespace}.")) {
                continue;
            }
            let base = match base {
                Base::Activity if meta_value(body, "android.app.lib_name").is_some() => {
                    if meta_value(body, "mobilesentinel.lib").is_some() {
                        Base::NativeRust
                    } else {
                        Base::Native
                    }
                }
                Base::Receiver
                    if meta_value(body, "android.app.device_admin").is_some()
                        || body.contains("\"android.app.device_admin\"") =>
                {
                    Base::AdminReceiver
                }
                other => other,
            };
            out.push(Component {
                class,
                base,
                lib: meta_value(body, "mobilesentinel.lib"),
            });
        }
    }
    out
}

/// Fully qualified names of the classes the app's own sources define.
pub fn defined_classes(source_dirs: &[PathBuf]) -> HashSet<String> {
    let mut found = HashSet::new();
    for dir in source_dirs {
        collect(dir, dir, &mut found);
    }
    found
}

fn collect(root: &Path, dir: &Path, found: &mut HashSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, found);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("java" | "kt")
        ) {
            if let Ok(rel) = path.with_extension("").strip_prefix(root) {
                let name = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(".");
                found.insert(name);
            }
        }
    }
}

/// Whether `name` can be a library name here (it becomes part of a JNI symbol).
pub fn valid_lib(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The Java files to write: (path under the source root, text). Errors when a component
/// needs a library and has none, or names an invalid one.
pub fn render(
    components: &[Component],
    defined: &HashSet<String>,
    default_lib: Option<&str>,
) -> Result<Vec<(PathBuf, String)>, String> {
    let mut files = Vec::new();
    let mut libs = Vec::new();
    for c in components.iter().filter(|c| !defined.contains(&c.class)) {
        let (package, simple) = c.class.rsplit_once('.').unwrap_or(("", &c.class));
        let text = if c.base == Base::Native {
            format!("package {package};\n\n/** Generated by build_sentinel: a screen drawn by native code. */\npublic class {simple} extends {} {{}}\n", c.base.class())
        } else {
            let lib = c.lib.as_deref().or(default_lib).ok_or_else(|| {
                format!("{} has no library: set rust_lib in sentinel.toml or mobilesentinel.lib meta-data", c.class)
            })?;
            if !valid_lib(lib) {
                return Err(format!(
                    "library name \"{lib}\" of {}: letters and digits only",
                    c.class
                ));
            }
            if !libs.contains(&lib) {
                libs.push(lib);
            }
            format!(
                "package {package};\n\n/** Generated by build_sentinel: its code is Rust, in lib{lib}.so. */\n\
                 public class {simple} extends {} {{\n    @Override\n    \
                 public Object rust(String event, Object self, android.content.Intent intent, int a, int b) {{\n        \
                 return com.mobilesentinel.rust.Lib_{lib}.event(\"{class}\", event, self, intent, a, b);\n    }}\n}}\n",
                c.base.class(),
                class = c.class,
            )
        };
        files.push((
            PathBuf::from(format!("{}.java", c.class.replace('.', "/"))),
            text,
        ));
    }
    for lib in libs {
        files.push((
            PathBuf::from(format!("com/mobilesentinel/rust/Lib_{lib}.java")),
            format!(
                "package com.mobilesentinel.rust;\n\n/** Generated by build_sentinel: the components handled by lib{lib}.so. */\n\
                 public final class Lib_{lib} {{\n    static {{\n        System.loadLibrary(\"{lib}\");\n    }}\n\n    \
                 private Lib_{lib}() {{}}\n\n    \
                 public static native Object event(String component, String event, Object self, Object intent, int a, int b);\n}}\n"
            ),
        ));
    }
    Ok(files)
}

fn resolve(name: &str, namespace: &str) -> String {
    if let Some(rest) = name.strip_prefix('.') {
        format!("{namespace}.{rest}")
    } else if name.contains('.') {
        name.to_string()
    } else {
        format!("{namespace}.{name}")
    }
}

fn attr(head: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = head.find(&key)? + key.len();
    let len = head[start..].find('"')?;
    Some(head[start..start + len].to_string())
}

/// The value (or resource) of the `<meta-data>` named `name` in `body`.
fn meta_value(body: &str, name: &str) -> Option<String> {
    let mut rest = body;
    while let Some(at) = rest.find("<meta-data") {
        let after = &rest[at..];
        let end = after.find('>').unwrap_or(after.len());
        let tag = &after[..end];
        if attr(tag, "android:name").as_deref() == Some(name) {
            return attr(tag, "android:value").or_else(|| attr(tag, "android:resource"));
        }
        rest = &after[end..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"<manifest>
    <application>
        <activity android:name=".MainActivity" android:exported="true">
            <meta-data android:name="android.app.lib_name" android:value="ui" />
        </activity>
        <activity android:name=".DevActivity" android:exported="true" />
        <activity android:name=".Screen">
            <meta-data android:name="android.app.lib_name" android:value="ui" />
            <meta-data android:name="mobilesentinel.lib" android:value="ui" />
        </activity>
        <activity-alias android:name=".Alias" android:targetActivity=".DevActivity" />
        <service android:name=".HostService" android:exported="false">
            <property android:name="x" android:value="y" />
        </service>
        <service android:name="dev.app.G0" android:process=":g0">
            <meta-data android:name="mobilesentinel.lib" android:value="core" />
        </service>
        <receiver android:name=".space.Admin" android:permission="android.permission.BIND_DEVICE_ADMIN">
            <meta-data android:name="android.app.device_admin" android:resource="@xml/device_admin" />
        </receiver>
        <receiver android:name=".space.Events" />
        <service android:name="com.mobilesentinel.SentinelForegroundService" />
    </application>
</manifest>"#;

    #[test]
    fn finds_the_apps_components_and_their_bases() {
        let found = components(MANIFEST, "dev.app");
        let by = |c: &str| found.iter().find(|x| x.class == c).cloned().unwrap();
        assert_eq!(found.len(), 7, "{found:?}");
        assert_eq!(by("dev.app.Screen").base, Base::NativeRust);
        assert_eq!(by("dev.app.MainActivity").base, Base::Native);
        assert_eq!(by("dev.app.DevActivity").base, Base::Activity);
        assert_eq!(by("dev.app.HostService").base, Base::Service);
        assert_eq!(by("dev.app.G0").lib.as_deref(), Some("core"));
        assert_eq!(by("dev.app.space.Admin").base, Base::AdminReceiver);
        assert_eq!(by("dev.app.space.Events").base, Base::Receiver);
        assert!(!found
            .iter()
            .any(|c| c.class.contains("Alias") || c.class.contains("Sentinel")));
    }

    #[test]
    fn renders_shims_and_one_bridge_per_library() {
        let found = components(MANIFEST, "dev.app");
        let mut defined = HashSet::new();
        defined.insert("dev.app.space.Events".to_string());
        let files = render(&found, &defined, Some("ui")).unwrap();
        let file = |p: &str| {
            files
                .iter()
                .find(|(path, _)| path == Path::new(p))
                .map(|(_, t)| t.clone())
        };
        assert!(
            file("dev/app/space/Events.java").is_none(),
            "a defined class is left alone"
        );
        assert!(file("dev/app/MainActivity.java")
            .unwrap()
            .contains("extends android.app.NativeActivity {}"));
        let host = file("dev/app/HostService.java").unwrap();
        assert!(host.contains("package dev.app;"));
        assert!(host.contains("extends com.mobilesentinel.rust.RustService"));
        assert!(host.contains("Lib_ui.event(\"dev.app.HostService\""));
        assert!(file("dev/app/G0.java").unwrap().contains("Lib_core.event("));
        assert!(file("dev/app/Screen.java")
            .unwrap()
            .contains("extends com.mobilesentinel.rust.RustNativeActivity"));
        assert!(file("com/mobilesentinel/rust/Lib_ui.java")
            .unwrap()
            .contains("System.loadLibrary(\"ui\")"));
        assert!(file("com/mobilesentinel/rust/Lib_core.java")
            .unwrap()
            .contains("public static native Object event"));
        assert_eq!(
            files
                .iter()
                .filter(|(p, _)| p.to_string_lossy().contains("Lib_"))
                .count(),
            2
        );
    }

    #[test]
    fn a_component_without_a_library_or_with_a_bad_name_is_an_error() {
        let found = components(MANIFEST, "dev.app");
        assert!(render(&found, &HashSet::new(), None)
            .unwrap_err()
            .contains("rust_lib"));
        assert!(render(&found, &HashSet::new(), Some("my_ui"))
            .unwrap_err()
            .contains("letters and digits"));
    }

    #[test]
    fn defined_classes_come_from_source_paths() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("dev/app")).unwrap();
        fs::write(dir.path().join("dev/app/Main.java"), "").unwrap();
        fs::write(dir.path().join("dev/app/Other.kt"), "").unwrap();
        fs::write(dir.path().join("dev/app/notes.txt"), "").unwrap();
        let found = defined_classes(&[dir.path().to_path_buf()]);
        assert!(found.contains("dev.app.Main") && found.contains("dev.app.Other"));
        assert_eq!(found.len(), 2);
    }
}

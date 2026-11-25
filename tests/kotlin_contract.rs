//! Contract between the Rust side and the Kotlin side, checked without a device.
//!
//! * Every Kotlin `external fun` has a Rust `Java_<class>_<name>` export, and the other way
//!   round, with the right class path. A mismatch otherwise shows up as an
//!   `UnsatisfiedLinkError` on the phone only. Three of them (`nativeStart`, `nativeOnBack` of the
//!   background service, `nativeRearm`) are implemented by the app: the SDK documents or generates
//!   their name instead of exporting them.
//! * The ring plan Rust serializes (`FireRequest`) has every field the Kotlin code reads, and is
//!   pinned in `tests/fixtures/ring-plan-*.json`, which the Kotlin tests read too.
//!   `UPDATE_FIXTURES=1 cargo test --all-features --test kotlin_contract` rewrites them.
#![cfg(firing_enabled)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use mobile_sentinel::FireRequest;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn files_under(dir: &Path, extension: &str, skip: &[&str], out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if path.is_dir() {
            if !skip.contains(&name.as_str()) {
                files_under(&path, extension, skip, out);
            }
        } else if name.ends_with(extension) {
            out.push(path);
        }
    }
}

/// Kotlin sources that ship in a module (not tests, not build output).
fn kotlin_sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    let android = root().join("android");
    for part in ["core", "caps"] {
        files_under(&android.join(part), ".kt", &["build", "test"], &mut files);
    }
    assert!(files.len() > 40, "found only {} Kotlin files", files.len());
    files
}

fn rust_sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    files_under(&root().join("src"), ".rs", &[], &mut files);
    files
}

/// The JNI symbol for a method of a class: `_` becomes `_1`, `.` becomes `_`.
fn jni_name(package: &str, class: &str, method: &str) -> String {
    let mangle = |s: &str| s.replace('_', "_1");
    format!(
        "Java_{}_{}_{}",
        package.split('.').map(mangle).collect::<Vec<_>>().join("_"),
        mangle(class),
        mangle(method)
    )
}

/// JNI name -> where it is declared, for every `external fun` of the Kotlin sources.
fn kotlin_externals() -> BTreeMap<String, String> {
    kotlin_externals_with_parameters().0
}

/// The same, and the parameter types each one declares.
fn kotlin_externals_with_parameters() -> (BTreeMap<String, String>, BTreeMap<String, Vec<String>>) {
    let mut found = BTreeMap::new();
    let mut signatures = BTreeMap::new();
    for file in kotlin_sources() {
        let text = fs::read_to_string(&file).unwrap();
        let package = text
            .lines()
            .find_map(|l| l.strip_prefix("package "))
            .unwrap_or_else(|| panic!("{} has no package", file.display()))
            .trim()
            .to_string();
        // The class is the last top-level `object` or `class` declared above the function.
        let mut class = None;
        for line in text.lines() {
            if !line.starts_with(' ') && !line.starts_with('\t') {
                let words: Vec<&str> = line.split_whitespace().collect();
                if let Some(i) = words
                    .iter()
                    .position(|w| *w == "object" || *w == "class" || *w == "interface")
                {
                    if let Some(name) = words.get(i + 1) {
                        let name: String = name
                            .chars()
                            .take_while(|c| c.is_alphanumeric() || *c == '_')
                            .collect();
                        class = Some(name);
                    }
                }
            }
            if let Some(at) = line.find("external fun ") {
                let rest = &line[at + "external fun ".len()..];
                let method: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let class = class
                    .clone()
                    .unwrap_or_else(|| panic!("external fun outside a class: {}", file.display()));
                found.insert(
                    jni_name(&package, &class, &method),
                    format!("{}: {class}.{method}", file.display()),
                );
                let params = rest
                    .split_once('(')
                    .and_then(|(_, r)| r.split_once(')'))
                    .map(|(p, _)| p)
                    .unwrap_or_default();
                let types = params
                    .split(',')
                    .filter_map(|p| p.split_once(':').map(|(_, t)| t.trim().to_string()))
                    .collect();
                signatures.insert(jni_name(&package, &class, &method), types);
            }
        }
    }
    (found, signatures)
}

/// Every `Java_...` name in the Rust sources, split into real `fn` exports and mentions in comments.
fn rust_jni_names() -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut exports, mut mentions) = (BTreeSet::new(), BTreeSet::new());
    for file in rust_sources() {
        for line in fs::read_to_string(&file).unwrap().lines() {
            let trimmed = line.trim_start();
            let mut rest = line;
            while let Some(at) = rest.find("Java_") {
                let name: String = rest[at..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                rest = &rest[at + name.len().max(1)..];
                // Names built from a macro argument (`Lib_1$name`) are not whole names.
                if rest.starts_with('$') {
                    continue;
                }
                if trimmed.starts_with("//") {
                    mentions.insert(name);
                } else if line.contains("fn Java_") {
                    exports.insert(name);
                }
            }
        }
    }
    (exports, mentions)
}

/// Implemented by the app in its own crate; the SDK exports a macro or documents the signature.
const IMPLEMENTED_BY_THE_APP: &[&str] = &[
    "Java_com_mobilesentinel_SentinelBackground_nativeStart",
    "Java_com_mobilesentinel_SentinelBackground_nativeOnBack",
    "Java_com_mobilesentinel_SentinelRearm_nativeRearm",
];

#[test]
fn every_kotlin_external_has_a_rust_export() {
    let (exports, mentions) = rust_jni_names();
    assert!(
        kotlin_externals().len() >= 12,
        "found only {:?}",
        kotlin_externals().keys()
    );
    let mut missing = Vec::new();
    for (name, at) in kotlin_externals() {
        if IMPLEMENTED_BY_THE_APP.contains(&name.as_str()) {
            assert!(
                mentions.contains(&name) || exports.contains(&name),
                "{name} ({at}) is implemented by the app, but the SDK no longer names it"
            );
        } else if !exports.contains(&name) {
            missing.push(format!("{name}  <- {at}"));
        }
    }
    assert!(
        missing.is_empty(),
        "Kotlin declares these but no Rust export has the name (wrong class path or method?):\n{}",
        missing.join("\n")
    );
}

#[test]
fn every_rust_export_has_a_kotlin_declaration() {
    let kotlin = kotlin_externals();
    let (exports, _) = rust_jni_names();
    assert!(exports.len() >= 8, "found only {exports:?}");
    let orphans: Vec<&String> = exports
        .iter()
        .filter(|n| !kotlin.contains_key(*n))
        .collect();
    assert!(
        orphans.is_empty(),
        "Rust exports these but no Kotlin `external fun` matches:\n{orphans:#?}"
    );
}

/// The parameter types of every Rust `Java_...` export, after the JNIEnv and the class.
fn rust_export_parameters() -> BTreeMap<String, Vec<String>> {
    let mut found = BTreeMap::new();
    for file in rust_sources() {
        let text = fs::read_to_string(&file).unwrap();
        let mut rest = text.as_str();
        while let Some(at) = rest.find("fn Java_") {
            let after = &rest[at + 3..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let Some(args) = after[name.len()..]
                .strip_prefix('(')
                .and_then(|a| a.split_once(')'))
            else {
                rest = after;
                continue;
            };
            let types: Vec<String> = args
                .0
                .split(',')
                .filter_map(|p| p.split_once(':').map(|(_, t)| t.trim().to_string()))
                .skip(2)
                .collect();
            found.insert(name, types);
            rest = after;
        }
    }
    found
}

/// A wrong parameter list compiles on both sides and crashes (or reads garbage) on the phone.
#[test]
fn external_signatures_match_the_rust_exports() {
    let (_, kotlin) = kotlin_externals_with_parameters();
    let rust = rust_export_parameters();
    let mut checked = 0;
    let mut wrong = Vec::new();
    for (name, kotlin_types) in &kotlin {
        let Some(rust_types) = rust.get(name) else {
            continue;
        };
        checked += 1;
        let same = kotlin_types.len() == rust_types.len()
            && kotlin_types.iter().zip(rust_types).all(|(k, r)| {
                let r = r.rsplit("::").next().unwrap_or(r);
                match k.as_str() {
                    "Double" => r == "jdouble",
                    "Float" => r == "jfloat",
                    "Long" => r == "jlong",
                    "Int" => r == "jint",
                    "Boolean" => r == "jboolean",
                    _ => true,
                }
            });
        if !same {
            wrong.push(format!(
                "{name}: Kotlin {kotlin_types:?}, Rust {rust_types:?}"
            ));
        }
    }
    assert!(checked >= 8, "compared only {checked} signatures");
    assert!(
        wrong.is_empty(),
        "parameter lists differ:
{}",
        wrong.join(
            "
"
        )
    );
}

/// The signature `RegisterNatives` is given (`NATIVE_METHODS` in `platform/android/jni.rs`) must be the
/// one the Kotlin declaration has. A wrong one fails at every launch, is logged as a warning, and the
/// method is found by name after all, which hides it.
#[test]
fn registered_jni_signatures_match_the_kotlin_declarations() {
    let (_, kotlin) = kotlin_externals_with_parameters();
    let jni = fs::read(root().join("src/platform/android/jni.rs")).unwrap();
    let jni = String::from_utf8_lossy(&jni);
    let table = jni
        .split_once("const NATIVE_METHODS")
        .and_then(|(_, rest)| rest.split_once("];"))
        .map(|(table, _)| table)
        .expect("NATIVE_METHODS not found in jni.rs");
    let literals: Vec<&str> = table
        .split('"')
        .enumerate()
        .filter_map(|(i, part)| (i % 2 == 1).then_some(part))
        .collect();
    assert!(
        literals.len() >= 24 && literals.len() % 3 == 0,
        "NATIVE_METHODS did not parse: {literals:?}"
    );
    let descriptor = |kotlin_type: &str| match kotlin_type {
        "Double" => "D".to_string(),
        "Float" => "F".to_string(),
        "Long" => "J".to_string(),
        "Int" => "I".to_string(),
        "Boolean" => "Z".to_string(),
        "String" => "Ljava/lang/String;".to_string(),
        "ByteArray" => "[B".to_string(),
        other => panic!("no JNI descriptor known for the Kotlin type {other}"),
    };
    let mut wrong = Vec::new();
    for entry in literals.chunks(3) {
        let (class_path, method, signature) = (entry[0], entry[1], entry[2]);
        let (package, class) = class_path.rsplit_once('/').unwrap();
        let name = jni_name(&package.replace('/', "."), class, method);
        let Some(types) = kotlin.get(&name) else {
            wrong.push(format!(
                "{class_path}.{method} is registered but not declared in Kotlin"
            ));
            continue;
        };
        let expected = format!(
            "({})V",
            types.iter().map(|t| descriptor(t)).collect::<String>()
        );
        if signature != expected {
            wrong.push(format!(
                "{class_path}.{method}: registered as {signature}, Kotlin declares {expected}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{}",
        wrong.join(
            "
"
        )
    );
}

#[test]
fn the_app_implemented_names_are_still_declared_in_kotlin() {
    let kotlin = kotlin_externals();
    for name in IMPLEMENTED_BY_THE_APP {
        assert!(kotlin.contains_key(*name), "{name} is no longer declared");
    }
}

// ---- The ring plan -----------------------------------------------------------------------

fn plan(full: bool) -> FireRequest {
    let mut p = FireRequest {
        instance_id: "alarm-1".into(),
        fresh: true,
        channel_id: "alarms".into(),
        channel_name: "Alarms".into(),
        title: "Wake up".into(),
        body: "07:00".into(),
        importance: 5,
        bypass_dnd: true,
        sound_uri: "file:///sounds/a.mp3".into(),
        fallback_sound_uris: vec![],
        audio_usage: "alarm".into(),
        audio_content_type: "sonification".into(),
        looping: true,
        vibrate: true,
        vibration_pattern: vec![],
        kiosk_mode: false,
        kiosk_block_home: true,
        kiosk_block_back: true,
        kiosk_block_recents: true,
        kiosk_block_volume: true,
        activity_fqcn: None,
        kiosk_debounce_ms: 50,
        kiosk_hide_status_bar: true,
        kiosk_hide_nav_bar: true,
        firing_full_screen: true,
        firing_banner_over_ui: true,
        fade_in_ms: 0,
        sound_delay_ms: 0,
        min_volume_percent: 0,
        ring_limit_secs: None,
        late_ring_window_ms: 0,
        backup_holds: 0,
        backup_hold_ms: 0,
        backup_text: String::new(),
        unlock_text: String::new(),
        unlock_button: String::new(),
        unlock_quiet_ms: 0,
    };
    if full {
        p.fallback_sound_uris = vec!["file:///sounds/b.mp3".into(), "silent://".into()];
        p.vibration_pattern = vec![0, 400, 200, 400];
        p.activity_fqcn = Some("com.example.RingActivity".into());
        p.fade_in_ms = 30_000;
        p.sound_delay_ms = 5_000;
        p.min_volume_percent = 60;
        p.ring_limit_secs = Some(1800);
        p.late_ring_window_ms = 600_000;
        p.backup_holds = 3;
        p.backup_hold_ms = 2_000;
        p.backup_text = "Hold to stop".into();
        p.unlock_text = "Unlock to stop".into();
        p.unlock_button = "Unlock".into();
        p.unlock_quiet_ms = 45_000;
    }
    p
}

fn fixture(name: &str) -> PathBuf {
    root().join("tests/fixtures").join(name)
}

fn check_fixture(name: &str, request: &FireRequest) {
    let actual = serde_json::to_value(request).unwrap();
    let path = fixture(name);
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        fs::write(&path, serde_json::to_string_pretty(&actual).unwrap() + "\n").unwrap();
    }
    let pinned: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
    )
    .unwrap();
    assert_eq!(
        pinned,
        actual,
        "the ring plan changed: the Kotlin side reads {}. If this is on purpose, check the Kotlin \
         readers, then UPDATE_FIXTURES=1",
        path.display()
    );
}

#[test]
fn the_full_ring_plan_matches_its_fixture() {
    check_fixture("ring-plan-full.json", &plan(true));
}

#[test]
fn the_minimal_ring_plan_matches_its_fixture() {
    check_fixture("ring-plan-minimal.json", &plan(false));
}

/// Keys the Kotlin code reads from a ring plan: `plan.optX("key")`, `plan.has("key")`, and the
/// boot receiver's `JSONObject(plan).optLong("key")`.
fn kotlin_plan_keys() -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for file in kotlin_sources() {
        for line in fs::read_to_string(&file).unwrap().lines() {
            let mut rest = line;
            while let Some(dot) = rest.find(".opt").or_else(|| rest.find(".has(")) {
                let before = &rest[..dot];
                let receiver_is_plan = before.ends_with("plan")
                    || before.ends_with("plan?")
                    || before.ends_with("JSONObject(plan)");
                let after = &rest[dot..];
                if let (true, Some(q)) = (receiver_is_plan, after.find("(\"")) {
                    let key: String = after[q + 2..].chars().take_while(|c| *c != '"').collect();
                    keys.insert(key);
                }
                rest = &rest[dot + 1..];
            }
        }
    }
    keys
}

#[test]
fn every_key_kotlin_reads_from_the_plan_exists_in_it() {
    let keys = kotlin_plan_keys();
    assert!(keys.len() >= 20, "found only {keys:?}");
    let serialized = serde_json::to_value(plan(true)).unwrap();
    let have: BTreeSet<&str> = serialized
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let missing: Vec<&String> = keys.iter().filter(|k| !have.contains(k.as_str())).collect();
    assert!(
        missing.is_empty(),
        "Kotlin reads plan keys that FireRequest does not serialize (renamed?): {missing:?}"
    );
}

// ---- The Kotlin methods Rust calls by name ------------------------------------------------------------------------------
//
// Rust calls them with `call_static_method(class, "name", "(descriptor)ret", ..)`. The JVM looks the method up by name and
// descriptor at the call, and a pending `NoSuchMethodError` there ends the process: a member of a Kotlin `object` without
// `@JvmStatic` is no static method (seen on a phone, 2026-11-26), and a changed parameter changes the descriptor. Both lists
// are read from the sources, none is kept by hand: the calls from `src/**/*.rs`, the members from the Kotlin modules.

/// (class path, method, descriptor, file) of every `call_static_method` whose class is one of ours, and how many were left out.
fn rust_static_calls() -> (Vec<(String, String, String, String)>, usize) {
    let (mut found, mut skipped) = (Vec::new(), 0);
    for file in rust_sources() {
        let text = fs::read_to_string(&file).unwrap();
        // `const NAME: &str = "com/mobilesentinel/Class";`
        let mut consts = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            let rest = line
                .strip_prefix("const ")
                .or_else(|| line.strip_prefix("pub const "));
            if let Some(rest) = rest {
                if let (Some((name, _)), Some(q)) = (rest.split_once(':'), rest.find('"')) {
                    let value: String = rest[q + 1..].chars().take_while(|c| *c != '"').collect();
                    consts.insert(name.trim().to_string(), value);
                }
            }
        }
        // The class of a call is the argument of the `with_jni_class(` before it.
        let mut classes: Vec<(usize, Option<String>)> = Vec::new();
        let mut at = 0;
        while let Some(i) = text[at..].find("with_jni_class(") {
            let start = at + i + "with_jni_class(".len();
            let arg: String = text[start..].chars().take_while(|c| *c != ',').collect();
            let arg = arg.trim();
            let class = match arg.strip_prefix('"') {
                Some(lit) => Some(lit.trim_end_matches('"').to_string()),
                None => consts.get(arg).cloned(),
            };
            classes.push((start, class));
            at = start;
        }
        let mut at = 0;
        while let Some(i) = text[at..].find("call_static_method(") {
            let start = at + i + "call_static_method(".len();
            at = start;
            // `class, "name", "(sig)ret"` with white space between the parts.
            let mut parts = text[start..].splitn(4, ',');
            let (Some(_receiver), Some(name), Some(sig)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let (name, sig) = (name.trim(), sig.trim());
            if !name.starts_with('"') || !sig.starts_with("\"(") {
                continue;
            }
            let name = name.trim_matches('"').to_string();
            let sig: String = sig
                .trim_start_matches('"')
                .chars()
                .take_while(|c| *c != '"')
                .collect();
            let class = classes
                .iter()
                .rev()
                .find(|(pos, _)| *pos < start)
                .and_then(|(_, c)| c.clone());
            match class {
                Some(class) if class.starts_with("com/mobilesentinel/") => {
                    found.push((class, name, sig, file.display().to_string()));
                }
                _ => skipped += 1,
            }
        }
    }
    (found, skipped)
}

/// The JVM descriptor of a Kotlin type as written in a signature, `None` for a class the file does not name.
fn kotlin_descriptor(
    ty: &str,
    imports: &BTreeMap<String, String>,
    package: &str,
    ours: &BTreeSet<String>,
) -> Option<String> {
    let ty = ty.trim().trim_end_matches('?').trim();
    let primitive = match ty {
        "Boolean" => "Z",
        "Int" => "I",
        "Long" => "J",
        "Float" => "F",
        "Double" => "D",
        "Byte" => "B",
        "Short" => "S",
        "Char" => "C",
        "Unit" => "V",
        "ByteArray" => "[B",
        "IntArray" => "[I",
        "LongArray" => "[J",
        "FloatArray" => "[F",
        "DoubleArray" => "[D",
        "BooleanArray" => "[Z",
        _ => "",
    };
    if !primitive.is_empty() {
        return Some(primitive.to_string());
    }
    if let Some(inner) = ty.strip_prefix("Array<").and_then(|t| t.strip_suffix('>')) {
        return Some(format!(
            "[{}",
            kotlin_descriptor(inner, imports, package, ours)?
        ));
    }
    let path = match ty {
        "String" => "java/lang/String".to_string(),
        "Any" => "java/lang/Object".to_string(),
        _ => match imports.get(ty) {
            Some(full) => full.replace('.', "/"),
            None if ours.contains(&format!("{}/{ty}", package.replace('.', "/"))) => {
                format!("{}/{ty}", package.replace('.', "/"))
            }
            None => return None,
        },
    };
    Some(format!("L{path};"))
}

/// Splits parameters at the commas outside of `<>` and `()`.
fn split_parameters(params: &str) -> Vec<String> {
    let (mut out, mut depth, mut current) = (Vec::new(), 0i32, String::new());
    for c in params.chars() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => depth -= 1,
            _ => {}
        }
        if c == ',' && depth == 0 {
            out.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

/// The class a top-level line declares, as a path (`com/mobilesentinel/Name`).
fn declared_class(line: &str, package: &str) -> Option<String> {
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let words: Vec<&str> = line.split_whitespace().collect();
    let i = words
        .iter()
        .position(|w| matches!(*w, "object" | "class" | "interface"))?;
    let name: String = words
        .get(i + 1)?
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    Some(format!("{}/{name}", package.replace('.', "/")))
}

/// One Kotlin member as the JVM sees it: (class path, JVM name, static, descriptor, `None` when a type is not known).
type Member = (String, String, bool, Option<String>);

fn kotlin_members() -> Vec<Member> {
    let mut parsed = Vec::new();
    let mut ours = BTreeSet::new();
    for file in kotlin_sources() {
        let text = fs::read_to_string(&file).unwrap();
        let package = text
            .lines()
            .find_map(|l| l.strip_prefix("package "))
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut imports = BTreeMap::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("import ") {
                let rest = rest.trim();
                if let Some((_, last)) = rest.rsplit_once('.') {
                    imports.insert(last.to_string(), rest.to_string());
                }
            }
        }
        ours.extend(text.lines().filter_map(|l| declared_class(l, &package)));
        parsed.push((text, package, imports));
    }
    let mut members = Vec::new();
    for (text, package, imports) in &parsed {
        let lines: Vec<&str> = text.lines().collect();
        let mut class: Option<String> = None;
        for (n, line) in lines.iter().enumerate() {
            if let Some(c) = declared_class(line, package) {
                class = Some(c);
            }
            let Some(class) = &class else { continue };
            // @JvmStatic on the line, or among the annotation lines right above it.
            let is_static = line.contains("@JvmStatic")
                || lines[..n]
                    .iter()
                    .rev()
                    .take_while(|l| l.trim_start().starts_with('@'))
                    .any(|l| l.contains("@JvmStatic"));
            if let Some(at) = line.find("fun ") {
                let rest = &line[at + 4..];
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                // The parameter list may go over several lines.
                let mut joined = String::from(rest);
                let mut k = n;
                while joined.matches('(').count() > joined.matches(')').count()
                    && k + 1 < lines.len()
                {
                    k += 1;
                    joined.push(' ');
                    joined.push_str(lines[k].trim());
                }
                let Some((_, after)) = joined.split_once('(') else {
                    continue;
                };
                let (mut depth, mut end) = (1, 0);
                for (i, c) in after.char_indices() {
                    match c {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                let (params, tail) = (&after[..end], &after[end + 1..]);
                let ret = tail
                    .trim_start()
                    .strip_prefix(':')
                    .map(|t| {
                        t.trim()
                            .split(['=', '{'])
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string()
                    })
                    .unwrap_or_else(|| "Unit".to_string());
                let mut descriptor = Some(String::from("("));
                for p in split_parameters(params) {
                    let Some((_, ty)) = p.split_once(':') else {
                        continue;
                    };
                    let ty = ty.split('=').next().unwrap_or("");
                    descriptor = match (descriptor, kotlin_descriptor(ty, imports, package, &ours))
                    {
                        (Some(d), Some(t)) => Some(d + &t),
                        _ => None,
                    };
                }
                let descriptor = descriptor.and_then(|d| {
                    kotlin_descriptor(&ret, imports, package, &ours).map(|r| format!("{d}){r}"))
                });
                members.push((class.clone(), name, is_static, descriptor));
                continue;
            }
            // A property: the JVM sees `getName()` and, for a var, `setName(..)`.
            let trimmed = line.trim_start();
            let after = ["var ", "val "].iter().find_map(|kw| {
                trimmed
                    .strip_prefix(kw)
                    .or_else(|| trimmed.split(&format!(" {kw}")).nth(1))
            });
            if let Some(after) = after {
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let ty = after
                    .split_once(':')
                    .map(|(_, t)| t.split('=').next().unwrap_or("").trim().to_string())
                    .unwrap_or_default();
                if !name.is_empty() && !ty.is_empty() {
                    let mut cap = name.clone();
                    cap[..1].make_ascii_uppercase();
                    let d = kotlin_descriptor(&ty, imports, package, &ours);
                    members.push((
                        class.clone(),
                        format!("get{cap}"),
                        is_static,
                        d.as_ref().map(|d| format!("(){d}")),
                    ));
                    members.push((
                        class.clone(),
                        format!("set{cap}"),
                        is_static,
                        d.as_ref().map(|d| format!("({d})V")),
                    ));
                }
            }
        }
    }
    members
}

#[test]
fn every_kotlin_method_rust_calls_by_name_exists_static_with_that_descriptor() {
    let (calls, skipped) = rust_static_calls();
    // The ones left out have a class that is not one of ours (an Android class) or a class the file does not name by a constant.
    assert!(
        calls.len() >= 100,
        "found only {} calls ({skipped} left out)",
        calls.len()
    );
    assert!(
        skipped <= 8,
        "{skipped} calls could not be given a class: the reading of the Rust sources broke"
    );
    let members = kotlin_members();
    assert!(
        members.len() > 200,
        "found only {} Kotlin members",
        members.len()
    );
    let mut wrong = Vec::new();
    let mut unchecked = 0;
    for (class, name, sig, file) in &calls {
        let same: Vec<&Member> = members
            .iter()
            .filter(|m| &m.0 == class && &m.1 == name)
            .collect();
        if same.is_empty() {
            wrong.push(format!(
                "{class}.{name}{sig}  <- {file}: no such member in the Kotlin sources"
            ));
        } else if !same.iter().any(|m| m.2) {
            wrong.push(format!(
                "{class}.{name}  <- {file}: not static (a member of an object needs @JvmStatic for the JNI call)"
            ));
        } else if !same
            .iter()
            .any(|m| m.2 && (m.3.as_deref() == Some(sig.as_str()) || m.3.is_none()))
        {
            let have: Vec<String> = same
                .iter()
                .filter(|m| m.2)
                .filter_map(|m| m.3.clone())
                .collect();
            wrong.push(format!(
                "{class}.{name}  <- {file}: Rust asks for {sig}, Kotlin has {have:?}"
            ));
        }
    }
    // A parameter of a type the test cannot name (no import, no class of ours) leaves that descriptor unchecked: the name and
    // @JvmStatic still are.
    for (class, name, _, _) in &calls {
        let all_unknown = members
            .iter()
            .filter(|m| &m.0 == class && &m.1 == name && m.2)
            .all(|m| m.3.is_none());
        unchecked += usize::from(all_unknown);
    }
    eprintln!(
        "{} Rust calls checked against the Kotlin sources ({skipped} left out, {unchecked} without a descriptor check)",
        calls.len()
    );
    assert!(
        unchecked <= 3,
        "{unchecked} calls have a Kotlin type the test cannot name"
    );
    assert!(
        wrong.is_empty(),
        "Rust calls these Kotlin methods and the JVM would not find them:\n{}",
        wrong.join("\n")
    );
}
